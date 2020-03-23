// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

//! Simplified implementation of the seminal [Epidemic Broadcast Trees] paper
//! and accompanying [HyParView] membership protocol.
//!
//! [Epidemic Broadcast Trees]: http://asc.di.fct.unl.pt/~jleitao/pdf/srds07-leitao.pdf
//! [HyParView]: http://asc.di.fct.unl.pt/~jleitao/pdf/dsn07-leitao.pdf

use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    iter,
    marker::PhantomData,
    net::SocketAddr,
    num::NonZeroU32,
    sync::{
        atomic::{self, AtomicBool},
        Arc,
    },
    time::Duration,
};

use futures::{
    channel::mpsc,
    io::{AsyncRead, AsyncWrite},
    lock::Mutex,
    sink::SinkExt,
    stream::StreamExt,
};
use futures_codec::{CborCodec, FramedRead, FramedWrite};
use futures_timer::Delay;
use governor::{Quota, RateLimiter};
use log::{info, trace, warn};
use rand::{seq::IteratorRandom, Rng};
use rand_pcg::Pcg64Mcg;
use serde::{Deserialize, Serialize};

use crate::{
    channel::Fanout,
    net::{connection::RemoteInfo, gossip::error::Error},
    peer::PeerId,
};

pub mod error;
pub mod rpc;
pub mod storage;
pub mod types;

pub use rpc::*;
pub use storage::*;
pub use types::*;

#[derive(Clone)]
pub enum ProtocolEvent<A> {
    SendAdhoc { to: PeerInfo, rpc: Rpc<A> },
    Connect { to: PeerInfo, hello: Rpc<A> },
    Disconnect(PeerId),
}

#[derive(Debug, Clone)]
pub struct MembershipParams {
    /// Maximum number of active connections.
    pub max_active: usize,
    /// The number of hops a [`Membership::ForwardJoin`] or
    /// [`Membership::Shuffle`] should be propageted.
    pub random_walk_length: usize,
    /// The maximum number of peers to include in a shuffle.
    pub shuffle_sample_size: usize,
    /// Interval in which to perform a shuffle.
    pub shuffle_interval: Duration,
    /// Interval in which to attempt to promote a passive peer.
    pub promote_interval: Duration,
}

impl Default for MembershipParams {
    fn default() -> Self {
        Self {
            max_active: 23,
            random_walk_length: 3,
            shuffle_sample_size: 7,
            shuffle_interval: Duration::from_secs(10),
            promote_interval: Duration::from_secs(5),
        }
    }
}

/// Placeholder for a datastructure representing the currently connected-to
/// peers
///
/// The number of peers is bounded -- when `insert`ing into an already full
/// `ConnectedPeers`, an existing connection is chosen at random, its write
/// stream is closed, and the corresponding `PeerId` is returned for upstream
/// connection management.
///
/// The random choice should be replaced by a weighted selection, which takes
/// metrics such as uptime, bandwidth, etc. into account.
#[derive(Clone, Default)]
struct ConnectedPeers<S, R> {
    max_peers: usize,
    rng: R,
    peers: HashMap<PeerId, S>,
}

impl<S, R> ConnectedPeers<S, R>
where
    S: Unpin,
    R: Rng,
{
    fn new(max_peers: usize, rng: R) -> Self {
        Self {
            max_peers,
            rng,
            peers: HashMap::default(),
        }
    }

    fn insert(&mut self, peer_id: PeerId, sink: S) -> Option<(PeerId, S)> {
        if !self.peers.contains_key(&peer_id) && self.peers.len() + 1 > self.max_peers {
            self.peers.insert(peer_id, sink);

            let eject = self
                .peers
                .keys()
                .choose(&mut self.rng)
                .expect("Iterator must contain at least 1 element, as per the if condition. qed")
                .clone();
            self.remove(&eject)
        } else {
            self.peers
                .insert(peer_id.clone(), sink)
                .map(|s| (peer_id, s))
        }
    }

    fn remove(&mut self, peer_id: &PeerId) -> Option<(PeerId, S)> {
        self.peers.remove(peer_id).map(|s| (peer_id.to_owned(), s))
    }

    fn random(&mut self) -> Option<(&PeerId, &mut S)> {
        self.peers.iter_mut().choose(&mut self.rng)
    }

    fn contains(&self, peer_id: &PeerId) -> bool {
        self.peers.contains_key(peer_id)
    }

    fn get_mut(&mut self, peer_id: &PeerId) -> Option<&mut S> {
        self.peers.get_mut(peer_id)
    }

    fn iter_mut(&mut self) -> impl Iterator<Item = (&PeerId, &mut S)> {
        self.peers.iter_mut()
    }
}

/// Placeholder for a datastructure with the following properties:
///
/// * Keeps a bounded number of `(PeerId, [SocketAddr])` pairs
/// * The list of addresses is itself bounded, and keeps track of the `n` most
///   recently seen "good" IP/port tuples
/// * The list is prioritised by recently-seen, and treats peer info relayed by
///   other peers (`Shuffle`d) with the least priority
#[derive(Clone, Default)]
struct KnownPeers<R> {
    peers: HashMap<PeerId, PeerInfo>,
    rng: R,
}

impl<R: Rng> KnownPeers<R> {
    fn new(rng: R) -> Self {
        Self {
            peers: HashMap::default(),
            rng,
        }
    }

    fn insert<I: IntoIterator<Item = PeerInfo>>(&mut self, peers: I) {
        for info in peers {
            let entry = self
                .peers
                .entry(info.peer_id.clone())
                .or_insert_with(|| info.clone());
            entry.seen_addrs = entry.seen_addrs.union(&info.seen_addrs).cloned().collect();
        }
    }

    fn random(&mut self) -> Option<PeerInfo> {
        self.peers.values().cloned().choose(&mut self.rng)
    }

    fn sample(&mut self, n: usize) -> Vec<PeerInfo> {
        self.peers
            .values()
            .cloned()
            .choose_multiple(&mut self.rng, n)
    }
}

type Codec<A> = CborCodec<Rpc<A>, Rpc<A>>;
type WriteStream<W, A> = FramedWrite<W, Codec<A>>;

type StorageErrorLimiter = RateLimiter<
    governor::state::direct::NotKeyed,
    governor::state::InMemoryState,
    governor::clock::DefaultClock,
>;

pub struct Protocol<S, A, R, W> {
    local_id: PeerId,
    local_ad: PeerAdvertisement,

    mparams: MembershipParams,

    prng: Pcg64Mcg,

    storage: S,
    storage_error_lim: Arc<StorageErrorLimiter>,

    subscribers: Fanout<ProtocolEvent<A>>,

    connected_peers: Arc<Mutex<ConnectedPeers<WriteStream<W, A>, Pcg64Mcg>>>,
    known_peers: Arc<Mutex<KnownPeers<Pcg64Mcg>>>,

    dropped: Arc<AtomicBool>,

    _marker: PhantomData<(A, R)>,
}

// `Clone` cannot be auto-derived, because the compiler can't see that `R` is
// only `PhantomData`, and `W` is behind an `Arc`. It places `Clone` constraints
// on `R` and `W`, which we can't (and don't want to) satisfy.
impl<S: Clone, A: Clone, R, W> Clone for Protocol<S, A, R, W> {
    fn clone(&self) -> Self {
        Self {
            local_id: self.local_id.clone(),
            local_ad: self.local_ad.clone(),
            mparams: self.mparams.clone(),
            prng: self.prng.clone(),
            storage: self.storage.clone(),
            storage_error_lim: self.storage_error_lim.clone(),
            subscribers: self.subscribers.clone(),
            connected_peers: self.connected_peers.clone(),
            known_peers: self.known_peers.clone(),
            dropped: self.dropped.clone(),
            _marker: self._marker,
        }
    }
}

impl<S, A, R, W> Protocol<S, A, R, W>
where
    S: LocalStorage<Update = A> + 'static,
    for<'de> A: Serialize + Deserialize<'de> + Clone + Debug + Send + Sync + 'static,
    R: AsyncRead + RemoteInfo + Unpin + Send + Sync + 'static,
    W: AsyncWrite + RemoteInfo + Unpin + Send + Sync + 'static,
{
    pub fn new(
        local_id: &PeerId,
        local_ad: PeerAdvertisement,
        mparams: MembershipParams,
        storage: S,
    ) -> Self {
        let prng = Pcg64Mcg::new(rand::random());
        let connected_peers = Arc::new(Mutex::new(ConnectedPeers::new(
            mparams.max_active,
            prng.clone(),
        )));
        let known_peers = Arc::new(Mutex::new(KnownPeers::new(prng.clone())));

        let storage_error_lim = Arc::new(RateLimiter::direct(Quota::per_second(unsafe {
            NonZeroU32::new_unchecked(5)
        })));

        let this = Self {
            local_id: local_id.clone(),
            local_ad,

            mparams,
            prng,

            storage,
            storage_error_lim,

            subscribers: Fanout::new(),

            connected_peers,
            known_peers,

            dropped: Arc::new(AtomicBool::new(false)),

            _marker: PhantomData,
        };

        this.clone().run_periodic_tasks();

        this
    }

    pub async fn announce(&self, have: A) {
        self.broadcast(
            Gossip::Have {
                origin: self.local_peer_info(),
                val: have,
            },
            None,
        )
        .await
    }

    pub async fn query(&self, want: A) {
        self.broadcast(
            Gossip::Want {
                origin: self.local_peer_info(),
                val: want,
            },
            None,
        )
        .await
    }

    pub(super) async fn subscribe(&self) -> mpsc::UnboundedReceiver<ProtocolEvent<A>> {
        self.subscribers.subscribe().await
    }

    pub(super) async fn outgoing(
        &self,
        recv: FramedRead<R, Codec<A>>,
        mut send: FramedWrite<W, Codec<A>>,
        hello: impl Into<Option<Rpc<A>>>,
    ) -> Result<(), Error> {
        let hello = hello
            .into()
            .unwrap_or_else(|| Membership::Join(self.local_ad.clone()).into());
        send.send(hello).await?;

        self.incoming(recv, send).await
    }

    pub(super) async fn incoming(
        &self,
        mut recv: FramedRead<R, Codec<A>>,
        send: FramedWrite<W, Codec<A>>,
    ) -> Result<(), Error> {
        let remote_id = recv.remote_peer_id().clone();
        // This should not be possible, as we prevent it in the TLS handshake.
        // Leaving it here regardless as a sanity check.
        if remote_id == self.local_id {
            return Err(Error::SelfConnection);
        }

        if let Some((ejected_peer, mut ejected_send)) =
            self.add_connected(remote_id.clone(), send).await
        {
            trace!(
                "{}: Ejecting connected peer {}",
                self.local_id,
                ejected_peer
            );
            let _ = ejected_send.close().await;
            // Note: if the ejected peer never sent us a `Join` or
            // `Neighbour`, it isn't behaving well, so we can forget about
            // it here. Otherwise, we should already have it in
            // `known_peers`.
            self.subscribers
                .emit(ProtocolEvent::Disconnect(ejected_peer))
                .await
        }

        while let Some(recvd) = recv.next().await {
            match recvd {
                Ok(rpc) => match rpc {
                    Rpc::Membership(msg) => {
                        self.handle_membership(&remote_id, recv.remote_addr(), msg)
                            .await?
                    },

                    Rpc::Gossip(msg) => self.handle_gossip(&remote_id, msg).await?,
                },

                Err(e) => {
                    warn!("{}: Recv error: {:?}", self.local_id, e);
                    break;
                },
            }
        }

        trace!(
            "{}: Recv stream from {} done, disconnecting",
            self.local_id,
            remote_id
        );
        self.remove_connected(&remote_id).await;

        Ok(())
    }

    async fn handle_membership(
        &self,
        remote_id: &PeerId,
        remote_addr: SocketAddr,
        msg: Membership,
    ) -> Result<(), Error> {
        use Membership::*;

        let make_peer_info = |ad: PeerAdvertisement| PeerInfo {
            peer_id: remote_id.clone(),
            advertised_info: ad,
            seen_addrs: vec![remote_addr].into_iter().collect(),
        };

        match msg {
            Join(ad) => {
                let peer_info = make_peer_info(ad);
                trace!(
                    "{}: Join with peer_info: peer_id: {}, advertised_info: {:?}, seen_addrs: {:?}",
                    self.local_id,
                    peer_info.peer_id,
                    peer_info.advertised_info,
                    peer_info.seen_addrs
                );

                self.add_known(iter::once(peer_info.clone())).await;
                self.broadcast(
                    ForwardJoin {
                        joined: peer_info,
                        ttl: self.mparams.random_walk_length,
                    },
                    remote_id,
                )
                .await
            },

            ForwardJoin { joined, ttl } => {
                trace!("{}: ForwardJoin: {:?}, {}", self.local_id, joined, ttl);
                if ttl == 0 {
                    self.connect(&joined, Neighbour(self.local_ad.clone()))
                        .await
                } else {
                    self.broadcast(
                        ForwardJoin {
                            joined,
                            ttl: ttl.saturating_sub(1),
                        },
                        remote_id,
                    )
                    .await
                }
            },

            Neighbour(ad) => {
                trace!("{}: Neighbour: {:?}", self.local_id, ad);
                self.add_known(iter::once(make_peer_info(ad))).await
            },

            Shuffle { origin, peers, ttl } => {
                trace!(
                    "{}: Shuffle: {:?}, {:?}, {}",
                    self.local_id,
                    origin,
                    peers,
                    ttl
                );
                // We're supposed to only remember shuffled peers at
                // the end of the random walk. Do it anyway for now.
                self.add_known(peers.clone()).await;

                if ttl > 0 {
                    let sample = self.sample_known().await;
                    self.send_adhoc(&origin, ShuffleReply { peers: sample })
                        .await
                } else {
                    let origin = if &origin.peer_id == remote_id {
                        make_peer_info(origin.advertised_info)
                    } else {
                        origin
                    };

                    self.broadcast(
                        Shuffle {
                            origin,
                            peers,
                            ttl: ttl.saturating_sub(1),
                        },
                        remote_id,
                    )
                    .await
                }
            },

            ShuffleReply { peers } => {
                trace!("{}: ShuffleReply: {:?}", self.local_id, peers);
                self.add_known(peers).await
            },
        }

        Ok(())
    }

    async fn handle_gossip(&self, remote_id: &PeerId, msg: Gossip<A>) -> Result<(), Error> {
        use Gossip::*;

        match msg {
            Have { origin, val } => {
                trace!("{}: {} has {:?}", self.local_id, origin.peer_id, val);

                let res = {
                    let remote_id = remote_id.clone();
                    let val = val.clone();
                    tokio::task::block_in_place(move || self.storage.put(&remote_id, val))
                };

                match res {
                    // `val` was new, and is now fetched to local storage. Let
                    // connected peers know they can now fetch it from us.
                    PutResult::Applied => {
                        info!("{}: Announcing applied value {:?}", self.local_id, val);
                        self.broadcast(
                            Have {
                                origin: self.local_peer_info(),
                                val,
                            },
                            remote_id,
                        )
                        .await
                    },

                    // Meh. Request retransmission.
                    PutResult::Error => {
                        info!("{}: Error applying {:?}", self.local_id, val);
                        // Forward in any case
                        self.broadcast(
                            Have {
                                origin,
                                val: val.clone(),
                            },
                            remote_id,
                        )
                        .await;
                        // Exit if we're getting too many errors
                        self.storage_error_lim
                            .check()
                            .map_err(|_| Error::StorageErrorRateLimitExceeded)?;
                        // Request retransmission
                        // This could be optimised be enqueuing `val`s and
                        // sending them in batch later (deduplicating)
                        self.broadcast(
                            Want {
                                origin: self.local_peer_info(),
                                val,
                            },
                            None,
                        )
                        .await
                    },

                    // Not interesting, forward to others
                    PutResult::Uninteresting => {
                        info!("{}: {:?} uninteresting", self.local_id, val);
                        self.broadcast(Have { origin, val }, remote_id).await
                    },

                    // We are up-to-date, don't do anything
                    PutResult::Stale => info!("{}: {:?} up to date", self.local_id, val),
                }
            },

            Want { origin, val } => {
                trace!("{}: {} wants {:?}", self.local_id, origin.peer_id, val);
                let have = {
                    let val = val.clone();
                    tokio::task::block_in_place(move || self.storage.ask(&val))
                };

                if have {
                    self.reply(
                        &remote_id.clone(),
                        Have {
                            origin: self.local_peer_info(),
                            val,
                        },
                    )
                    .await
                } else {
                    self.broadcast(Want { origin, val }, remote_id).await
                }
            },
        }

        Ok(())
    }

    async fn add_connected(
        &self,
        peer_id: PeerId,
        out: FramedWrite<W, Codec<A>>,
    ) -> Option<(PeerId, FramedWrite<W, Codec<A>>)> {
        self.connected_peers.lock().await.insert(peer_id, out)
    }

    async fn remove_connected(&self, peer_id: &PeerId) {
        if let Some((_, mut stream)) = self.connected_peers.lock().await.remove(peer_id) {
            let _ = stream.close().await;
        }
    }

    async fn add_known<I: IntoIterator<Item = PeerInfo>>(&self, peers: I) {
        self.known_peers.lock().await.insert(peers)
    }

    async fn sample_known(&self) -> Vec<PeerInfo> {
        self.known_peers
            .lock()
            .await
            .sample(self.mparams.shuffle_sample_size)
    }

    fn run_periodic_tasks(self) {
        let this = self.clone();
        tokio::spawn(async move {
            loop {
                if this.dropped.load(atomic::Ordering::Relaxed) {
                    break;
                }
                Delay::new(this.mparams.shuffle_interval).await;
                this.shuffle().await;
            }
        });

        tokio::spawn(async move {
            loop {
                if self.dropped.load(atomic::Ordering::Relaxed) {
                    break;
                }
                Delay::new(self.mparams.promote_interval).await;
                self.promote_random().await;
            }
        });
    }

    async fn shuffle(&self) {
        trace!("{}: Initiating shuffle", self.local_id);
        let mut connected = self.connected_peers.lock().await;
        if let Some((recipient, recipient_send)) = connected.random() {
            // Note: we should pick from the connected peers first, padding with
            // passive ones up to `shuffle_sample_size`. However, we don't track
            // the advertised info for those, as it will be available only later
            // (if and when they send it to us). Since in the latter case we
            // _will_ insert into `known_peers`, it doesn't really matter. The
            // `KnownPeers` type should make a weighted random choice
            // eventually.
            let sample = self.sample_known().await;
            if !sample.is_empty() {
                trace!(
                    "{}: shuffling sample {:?} with {}",
                    self.local_id,
                    sample,
                    recipient
                );
                recipient_send
                    .send(
                        Membership::Shuffle {
                            origin: self.local_peer_info(),
                            peers: sample,
                            ttl: self.mparams.random_walk_length,
                        }
                        .into(),
                    )
                    .await
                    .unwrap_or_else(|e| warn!("Failed to send shuffle to {}: {:?}", recipient, e))
            } else {
                trace!("{}: nothing to shuffle", self.local_id);
            }
        } else {
            trace!("{}: No connected peers to shuffle with", self.local_id);
        }
    }

    async fn promote_random(&self) {
        trace!("{}: Initiating random promotion", self.local_id);
        if let Some(candidate) = self.known_peers.lock().await.random() {
            if !self
                .connected_peers
                .lock()
                .await
                .contains(&candidate.peer_id)
            {
                trace!("{}: Promoting: {}", self.local_id, candidate.peer_id);
                self.connect(&candidate, Membership::Neighbour(self.local_ad.clone()))
                    .await
            }
        }
    }

    /// Send an [`Rpc`] to all currently connected peers, except `excluding`
    async fn broadcast<'a, M, X>(&self, rpc: M, excluding: X)
    where
        M: Into<Rpc<A>>,
        X: Into<Option<&'a PeerId>>,
    {
        let rpc = rpc.into();
        let excluding = excluding.into();

        let mut connected_peers = self.connected_peers.lock().await;
        futures::stream::iter(
            connected_peers
                .iter_mut()
                .filter(|(peer_id, _)| Some(*peer_id) != excluding),
        )
        .for_each_concurrent(None, |(peer, out)| {
            let rpc = rpc.clone();
            async move {
                trace!("{}: Broadcast {:?} to {}", self.local_id, rpc, peer);
                // If this returns an error, it is likely the receiving end has
                // stopped working, too. Hence, we don't need to propagate
                // errors here. This statement will need some empirical
                // evidence.
                if let Err(e) = out.send(rpc).await {
                    warn!(
                        "{}: Failed to send broadcast message to {}: {:?}",
                        self.local_id, peer, e
                    )
                }
            }
        })
        .await
    }

    async fn reply<M: Into<Rpc<A>>>(&self, to: &PeerId, rpc: M) {
        let rpc = rpc.into();
        futures::stream::iter(self.connected_peers.lock().await.get_mut(to))
            .for_each(|out| {
                let rpc = rpc.clone();
                async move {
                    trace!("{}: Reply with {:?} to {}", self.local_id, rpc, to);
                    if let Err(e) = out.send(rpc).await {
                        warn!("{}: Failed to reply to {}: {:?}", self.local_id, to, e);
                    }
                }
            })
            .await
    }

    /// Try to establish an ad-hoc connection to `peer`, and send it `rpc`
    async fn send_adhoc<M: Into<Rpc<A>>>(&self, peer: &PeerInfo, rpc: M) {
        self.subscribers
            .emit(ProtocolEvent::SendAdhoc {
                to: peer.clone(),
                rpc: rpc.into(),
            })
            .await
    }

    /// Try to establish a persistent to `peer` with initial `rpc`
    async fn connect<M: Into<Rpc<A>>>(&self, peer: &PeerInfo, rpc: M) {
        self.subscribers
            .emit(ProtocolEvent::Connect {
                to: peer.clone(),
                hello: rpc.into(),
            })
            .await
    }

    fn local_peer_info(&self) -> PeerInfo {
        PeerInfo {
            peer_id: self.local_id.clone(),
            advertised_info: self.local_ad.clone(),
            seen_addrs: HashSet::with_capacity(0),
        }
    }
}

impl<S, A, R, W> Drop for Protocol<S, A, R, W> {
    fn drop(&mut self) {
        self.dropped.store(true, atomic::Ordering::Relaxed)
    }
}
