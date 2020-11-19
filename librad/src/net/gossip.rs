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
    hash::Hash,
    iter,
    marker::PhantomData,
    num::NonZeroU32,
    sync::{
        atomic::{self, AtomicUsize},
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
use futures_codec::{Framed, FramedRead, FramedWrite};
use futures_timer::Delay;
use governor::{Quota, RateLimiter};
use minicbor::{Decode, Encode};
use rand::{seq::IteratorRandom, Rng};
use rand_pcg::Pcg64Mcg;
use tracing_futures::Instrument;

use crate::{
    internal::channel::Fanout,
    net::{
        codec::CborCodec,
        connection::{self, AsAddr, RemoteInfo},
        gossip::error::Error,
        upgrade::{self, Upgraded},
    },
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
pub enum ProtocolEvent<Addr, Payload>
where
    Addr: Clone + PartialEq + Eq + Hash,
{
    Control(Control<Addr, Payload>),
    Info(Info<Addr, Payload>),
    Membership(MembershipInfo<Addr>),
}

#[derive(Clone)]
pub enum Control<Addr, Payload>
where
    Addr: Clone + PartialEq + Eq + Hash,
{
    SendAdhoc {
        to: PeerInfo<Addr>,
        rpc: Rpc<Addr, Payload>,
    },
    Connect {
        to: PeerInfo<Addr>,
    },
    Disconnect(PeerId),
}

#[derive(Clone, Debug)]
pub enum Info<Addr, A>
where
    Addr: Clone + PartialEq + Eq + Hash,
{
    Has(Has<Addr, A>),
}

#[derive(Clone, Debug)]
pub struct Has<Addr, A>
where
    Addr: Clone + PartialEq + Eq + Hash,
{
    pub provider: PeerInfo<Addr>,
    pub val: A,
}

#[derive(Clone, Debug)]
pub enum MembershipInfo<Addr>
where
    Addr: Clone + Eq + Hash,
{
    Join(PeerAdvertisement<Addr>),
    Neighbour(PeerAdvertisement<Addr>),
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
            shuffle_interval: Duration::from_secs(30),
            promote_interval: Duration::from_secs(20),
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

            let (eject, _) = self
                .random()
                .expect("Iterator must contain at least 1 element, as per the if condition. qed");
            self.remove(eject)
        } else {
            self.peers.insert(peer_id, sink).map(|s| (peer_id, s))
        }
    }

    fn remove(&mut self, peer_id: PeerId) -> Option<(PeerId, S)> {
        self.peers.remove(&peer_id).map(|s| (peer_id, s))
    }

    fn random(&mut self) -> Option<(PeerId, &mut S)> {
        self.peers
            .iter_mut()
            .choose(&mut self.rng)
            .map(|(k, s)| (*k, s))
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

    fn len(&self) -> usize {
        self.peers.len()
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
struct KnownPeers<Addr, Rng>
where
    Addr: Clone + PartialEq + Eq + Hash,
{
    peers: HashMap<PeerId, PeerInfo<Addr>>,
    rng: Rng,
}

impl<Addr, R> KnownPeers<Addr, R>
where
    Addr: Clone + PartialEq + Eq + Hash,
    R: Rng,
{
    fn new(rng: R) -> Self {
        Self {
            peers: HashMap::default(),
            rng,
        }
    }

    fn insert<I: IntoIterator<Item = PeerInfo<Addr>>>(&mut self, peers: I) {
        for info in peers {
            let entry = self
                .peers
                .entry(info.peer_id)
                .or_insert_with(|| info.clone());
            entry.seen_addrs = entry.seen_addrs.union(&info.seen_addrs).cloned().collect();
        }
    }

    fn random(&mut self) -> Option<PeerInfo<Addr>> {
        self.peers.values().cloned().choose(&mut self.rng)
    }

    fn sample(&mut self, n: usize) -> Vec<PeerInfo<Addr>> {
        self.peers
            .values()
            .cloned()
            .choose_multiple(&mut self.rng, n)
    }
}

pub type Codec<A, P> = CborCodec<Rpc<A, P>, Rpc<A, P>>;
type WriteStream<W, A, P> = FramedWrite<W, Codec<A, P>>;
type ConnectedPeersImpl<W, A, P, R> = ConnectedPeers<WriteStream<W, A, P>, R>;
type StorageErrorLimiter = RateLimiter<
    governor::state::direct::NotKeyed,
    governor::state::InMemoryState,
    governor::clock::DefaultClock,
>;

pub struct Protocol<Storage, Broadcast, Addr, R, W>
where
    Addr: Clone + PartialEq + Eq + Hash,
{
    local_id: PeerId,
    local_ad: PeerAdvertisement<Addr>,

    mparams: MembershipParams,

    storage: Storage,
    storage_error_lim: Arc<StorageErrorLimiter>,

    subscribers: Fanout<ProtocolEvent<Addr, Broadcast>>,

    connected_peers: Arc<Mutex<ConnectedPeersImpl<W, Addr, Broadcast, Pcg64Mcg>>>,
    known_peers: Arc<Mutex<KnownPeers<Addr, Pcg64Mcg>>>,

    ref_count: Arc<AtomicUsize>,

    _marker: PhantomData<R>,
}

// `Clone` cannot be auto-derived, because the compiler can't see that `R` is
// only `PhantomData`, and `W` is behind an `Arc`. It places `Clone` constraints
// on `R` and `W`, which we can't (and don't want to) satisfy.
impl<Storage, Broadcast, Addr, R, W> Clone for Protocol<Storage, Broadcast, Addr, R, W>
where
    Storage: Clone,
    Broadcast: Clone,
    Addr: Clone + PartialEq + Eq + Hash,
{
    fn clone(&self) -> Self {
        const MAX_REFCOUNT: usize = (isize::MAX) as usize;

        let old_count = self.ref_count.fetch_add(1, atomic::Ordering::Relaxed);
        // See source docs of `std::sync::Arc`
        if old_count > MAX_REFCOUNT {
            eprintln!("Fatal: max refcount on gossip::Protocol exceeded");
            core::intrinsics::abort()
        }

        Self {
            local_id: self.local_id,
            local_ad: self.local_ad.clone(),
            mparams: self.mparams.clone(),
            storage: self.storage.clone(),
            storage_error_lim: self.storage_error_lim.clone(),
            subscribers: self.subscribers.clone(),
            connected_peers: self.connected_peers.clone(),
            known_peers: self.known_peers.clone(),
            ref_count: self.ref_count.clone(),
            _marker: self._marker,
        }
    }
}

impl<Storage, Broadcast, Addr, R, W> Protocol<Storage, Broadcast, Addr, R, W>
where
    Storage: LocalStorage<Update = Broadcast> + 'static,

    for<'de> Broadcast: Encode + Decode<'de> + Clone + Debug + Send + Sync + 'static,
    for<'de> Addr:
        Encode + Decode<'de> + Clone + Debug + Hash + PartialEq + Eq + Send + Sync + 'static,

    R: AsyncRead + RemoteInfo + Unpin + Send + Sync + 'static,
    W: AsyncWrite + RemoteInfo + Unpin + Send + Sync + 'static,

    <R as RemoteInfo>::Addr: AsAddr<Addr>,
    <W as RemoteInfo>::Addr: AsAddr<Addr>,
{
    pub fn new(
        local_id: PeerId,
        local_ad: PeerAdvertisement<Addr>,
        mparams: MembershipParams,
        storage: Storage,
    ) -> Self {
        let span = tracing::trace_span!("Protocol", local.id = %local_id);
        let _guard = span.enter();

        let prng = Pcg64Mcg::new(rand::random());
        let connected_peers = Arc::new(Mutex::new(ConnectedPeers::new(
            mparams.max_active,
            prng.clone(),
        )));
        let known_peers = Arc::new(Mutex::new(KnownPeers::new(prng)));

        let storage_error_lim = Arc::new(RateLimiter::direct(Quota::per_second(unsafe {
            NonZeroU32::new_unchecked(5)
        })));

        let this = Self {
            local_id,
            local_ad,

            mparams,

            storage,
            storage_error_lim,

            subscribers: Fanout::new(),

            connected_peers,
            known_peers,

            ref_count: Arc::new(AtomicUsize::new(0)),

            _marker: PhantomData,
        };

        // Spawn periodic tasks, ensuring they complete when the last reference
        // to `this` is dropped.
        {
            let shuffle = this.clone();
            let promotion = this.clone();
            // We got two clones, so if the ref_count goes below that, we're the
            // only ones holding on to a reference of `this`.
            let ref_count = 2;
            tokio::spawn(async move {
                loop {
                    if shuffle.ref_count.load(atomic::Ordering::Relaxed) < ref_count {
                        tracing::trace!("Stopping periodic shuffle task");
                        break;
                    }
                    Delay::new(shuffle.mparams.shuffle_interval).await;
                    shuffle.shuffle().await;
                }
            });
            tokio::spawn(async move {
                loop {
                    if promotion.ref_count.load(atomic::Ordering::Relaxed) < ref_count {
                        tracing::trace!("Stopping periodic promotion task");
                        break;
                    }
                    Delay::new(promotion.mparams.promote_interval).await;
                    promotion.promote_random().await;
                }
            });
        }

        this
    }

    pub fn peer_id(&self) -> PeerId {
        self.local_id
    }

    pub async fn is_connected(&self) -> bool {
        self.connected_peers.lock().await.len() > 0
    }

    pub async fn announce(&self, have: Broadcast) {
        let span = tracing::trace_span!("Protocol::announce", local.id = %self.local_id);

        self.broadcast(
            Gossip::Have {
                origin: self.local_peer_info(),
                val: have,
            },
            None,
        )
        .instrument(span)
        .await
    }

    pub async fn query(&self, want: Broadcast) {
        let span = tracing::trace_span!("Protocol::query", local.id = %self.local_id);

        self.broadcast(
            Gossip::Want {
                origin: self.local_peer_info(),
                val: want,
            },
            None,
        )
        .instrument(span)
        .await
    }

    pub(super) async fn subscribe(
        &self,
    ) -> mpsc::UnboundedReceiver<ProtocolEvent<Addr, Broadcast>> {
        self.subscribers.subscribe().await
    }

    pub(super) async fn outgoing<Stream>(
        &self,
        s: Upgraded<upgrade::Gossip, Stream>,
        hello: impl Into<Option<Rpc<Addr, Broadcast>>>,
    ) -> Result<(), Error>
    where
        Stream: connection::Stream<Read = R, Write = W>,
    {
        let hello = match hello.into() {
            Some(rpc) => rpc,
            None => if self.is_connected().await {
                Membership::Neighbour(self.local_ad.clone())
            } else {
                Membership::Join(self.local_ad.clone())
            }
            .into(),
        };

        let mut s = Framed::new(s, Codec::new());
        s.send(hello).await?;

        self.incoming(s.release().0).await
    }

    pub(super) async fn incoming<Stream>(
        &self,
        s: Upgraded<upgrade::Gossip, Stream>,
    ) -> Result<(), Error>
    where
        Stream: connection::Stream<Read = R, Write = W>,
    {
        let span = tracing::trace_span!("Protocol::incoming", local.id = %self.local_id);

        async move {
            let (recv, send) = s.into_stream().split();
            let mut recv = FramedRead::new(recv, Codec::new());
            let send = FramedWrite::new(send, Codec::new());

            let remote_id = recv.remote_peer_id();
            // This should not be possible, as we prevent it in the TLS handshake.
            // Leaving it here regardless as a sanity check.
            if remote_id == self.local_id {
                return Err(Error::SelfConnection);
            }

            tracing::error!("Check peer exists for remote.id = {}", remote_id);

            if let Some((ejected_peer, mut ejected_send)) =
                self.add_connected(remote_id, send).await
            {
                tracing::error!(
                    msg = "Ejecting connected peer",
                    peer = %ejected_peer,
                );
                let this = self.clone();
                tokio::spawn(async move {
                    let _ = ejected_send.close().await;
                    // Note: if the ejected peer never sent us a `Join` or
                    // `Neighbour`, it isn't behaving well, so we can forget about
                    // it here. Otherwise, we should already have it in
                    // `known_peers`.
                    this.subscribers
                        .emit(ProtocolEvent::Control(Control::Disconnect(ejected_peer)))
                        .await
                });
            }

            tracing::error!("Handling gossip incoming from remote.id = {}", remote_id);

            while let Some(recvd) = recv.next().await {
                match recvd {
                    Ok(rpc) => match rpc {
                        Rpc::Membership(msg) => {
                            if let Err(err) = self
                                .handle_membership(remote_id, recv.remote_addr().as_addr(), msg)
                                .await
                            {
                                self.remove_connected(remote_id).await;
                                return Err(err);
                            }
                        },

                        Rpc::Gossip(msg) => {
                            if let Err(err) = self.handle_gossip(remote_id, msg).await {
                                self.remove_connected(remote_id).await;
                                return Err(err);
                            }
                        },
                    },

                    Err(e) => {
                        tracing::error!("Recv error: {:?} for {}", e, remote_id);
                        break;
                    },
                }
            }

            tracing::error!(msg = "Recv stream is done, disconnecting");
            self.remove_connected(remote_id).await;

            Ok(())
        }
        .instrument(span)
        .await
    }

    async fn handle_membership(
        &self,
        remote_id: PeerId,
        remote_addr: Addr,
        msg: Membership<Addr>,
    ) -> Result<(), Error> {
        use Membership::*;

        let make_peer_info = |ad: PeerAdvertisement<Addr>| PeerInfo {
            peer_id: remote_id,
            advertised_info: ad,
            seen_addrs: vec![remote_addr].into_iter().collect(),
        };

        match msg {
            Join(ad) => {
                let peer_info = make_peer_info(ad.clone());
                tracing::trace!(
                    msg = "Join with peer information",
                    peer.info.advertised = ?peer_info.advertised_info,
                    peer.info.addrs = ?peer_info.seen_addrs,
                );

                self.add_known(iter::once(peer_info.clone())).await;
                self.broadcast(
                    ForwardJoin {
                        joined: peer_info,
                        ttl: self.mparams.random_walk_length,
                    },
                    remote_id,
                )
                .await;

                self.subscribers
                    .emit(ProtocolEvent::Membership(MembershipInfo::Join(ad)))
                    .await;
            },

            ForwardJoin { joined, ttl } => {
                tracing::trace!(msg = "ForwardJoin", joined = ?joined, ttl = ?ttl);
                if ttl == 0 {
                    self.connect(&joined).await
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
                tracing::trace!(msg = "Neighbour advertisement", peer.info.advertised = ?ad);
                self.add_known(iter::once(make_peer_info(ad.clone()))).await;

                self.subscribers
                    .emit(ProtocolEvent::Membership(MembershipInfo::Neighbour(ad)))
                    .await;
            },

            Shuffle { origin, peers, ttl } => {
                tracing::trace!(msg = "Shuffle", origin = ?origin, peer.neighbours = ?peers, peer.ttl = ttl);
                // We're supposed to only remember shuffled peers at
                // the end of the random walk. Do it anyway for now.
                self.add_known(peers.clone()).await;

                if ttl == 0 {
                    let sample = self.sample_known().await;
                    self.send_adhoc(&origin, ShuffleReply { peers: sample })
                        .await
                } else {
                    let origin = if origin.peer_id == remote_id {
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
                tracing::trace!(msg = "ShuffleReply", peer.neighbours = ?peers);
                self.add_known(peers).await
            },
        }

        Ok(())
    }

    async fn handle_gossip(
        &self,
        remote_id: PeerId,
        msg: Gossip<Addr, Broadcast>,
    ) -> Result<(), Error> {
        use Gossip::*;

        let span = tracing::trace_span!("Protocol::handle_gossip");

        async move {
            match msg {
                Have { origin, val } => {
                    tracing::trace!(origin.peer.id = %origin.peer_id, origin.value=?val, "Have");

                    self.subscribers
                        .emit(ProtocolEvent::Info(Info::Has(Has {
                            provider: origin.clone(),
                            val: val.clone(),
                        })))
                        .await;

                    match self.storage.put(remote_id, val.clone()).await {
                        // `val` was new, and is now fetched to local storage.
                        // Let connected peers know they can now fetch it from
                        // us.
                        PutResult::Applied(val) => {
                            tracing::info!(value = ?val, "Announcing applied value");

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
                            tracing::info!(value = ?val, "Error applying value");

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
                            tracing::info!(value = ?val, "Value is uninteresting");

                            self.broadcast(Have { origin, val }, remote_id).await
                        },

                        // We are up-to-date, don't do anything
                        PutResult::Stale => {
                            tracing::info!(value = ?val, "Value is up to date");
                        },
                    }
                },

                Want { origin, val } => {
                    tracing::trace!(origin.peer.id = %origin.peer_id, origin.value = ?val, "Want");

                    let have = self.storage.ask(val.clone()).await;
                    if have {
                        self.reply(
                            &remote_id,
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
        .instrument(span)
        .await
    }

    async fn add_connected(
        &self,
        peer_id: PeerId,
        out: FramedWrite<W, Codec<Addr, Broadcast>>,
    ) -> Option<(PeerId, FramedWrite<W, Codec<Addr, Broadcast>>)> {
        self.connected_peers.lock().await.insert(peer_id, out)
    }

    async fn remove_connected(&self, peer_id: PeerId) {
        if let Some((_, mut stream)) = self.connected_peers.lock().await.remove(peer_id) {
            let _ = stream.close().await;
        }
    }

    async fn add_known<I: IntoIterator<Item = PeerInfo<Addr>>>(&self, peers: I) {
        self.known_peers.lock().await.insert(
            peers
                .into_iter()
                .filter(|info| info.peer_id != self.peer_id()),
        )
    }

    async fn sample_known(&self) -> Vec<PeerInfo<Addr>> {
        self.known_peers
            .lock()
            .await
            .sample(self.mparams.shuffle_sample_size)
    }

    async fn shuffle(&self) {
        tracing::trace!("Initiating shuffle");
        let peer = {
            let mut peers = self.connected_peers.lock().await;
            match peers.random() {
                Some((recipient, _)) => Some(recipient),
                None => None,
            }
        };
        if let Some(recipient) = peer {
            // Note: we should pick from the connected peers first, padding with
            // passive ones up to `shuffle_sample_size`. However, we don't track
            // the advertised info for those, as it will be available only later
            // (if and when they send it to us). Since in the latter case we
            // _will_ insert into `known_peers`, it doesn't really matter. The
            // `KnownPeers` type should make a weighted random choice
            // eventually.
            let sample = self.sample_known().await;
            if !sample.is_empty() {
                tracing::trace!(
                    msg = "Shuffling sample",
                    shuffle.sample = ?sample,
                    shuffle.recipient = %recipient,
                );
                let mut peers = self
                    .connected_peers
                    .try_lock()
                    .expect("Unable to get connected_peers lock");
                let recipient_send = peers.get_mut(&recipient).expect("Picked recipient is gone");

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
                    .unwrap_or_else(|e| {
                        tracing::warn!("Failed to send shuffle to {}: {:?}", recipient, e)
                    })
            } else {
                tracing::trace!("Nothing to shuffle");
            }
        } else {
            tracing::trace!("No connected peers to shuffle with");
        }
    }

    async fn promote_random(&self) {
        tracing::trace!(msg = "Initiating random promotion",);
        if let Some(candidate) = self.known_peers.lock().await.random() {
            if !self
                .connected_peers
                .lock()
                .await
                .contains(&candidate.peer_id)
            {
                tracing::trace!(msg = "Promoting candidate", candidate.id = %candidate.peer_id);
                self.connect(&candidate).await
            }
        }
    }

    /// Send an [`Rpc`] to all currently connected peers, except `excluding`
    async fn broadcast<M, X>(&self, rpc: M, excluding: X)
    where
        M: Into<Rpc<Addr, Broadcast>>,
        X: Into<Option<PeerId>>,
    {
        let rpc = rpc.into();
        let excluding = excluding.into();

        let mut connected_peers = self.connected_peers.lock().await;
        futures::stream::iter(
            connected_peers
                .iter_mut()
                .filter(|(peer_id, _)| Some(*peer_id) != excluding.as_ref()),
        )
        .for_each_concurrent(None, |(peer, out)| {
            let rpc = rpc.clone();
            async move {
                tracing::trace!(msg = "Broadcast", broadcast.rpc = ?rpc, broadcast.peer = %peer);
                // If this returns an error, it is likely the receiving end has
                // stopped working, too. Hence, we don't need to propagate
                // errors here. This statement will need some empirical
                // evidence.
                if let Err(e) = out.send(rpc).await {
                    tracing::warn!(
                        "{}: Failed to send broadcast message to {}: {:?}",
                        self.local_id,
                        peer,
                        e
                    )
                }
            }
        })
        .await
    }

    async fn reply<M: Into<Rpc<Addr, Broadcast>>>(&self, to: &PeerId, rpc: M) {
        let rpc = rpc.into();
        futures::stream::iter(self.connected_peers.lock().await.get_mut(&to))
            .for_each(|out| {
                let rpc = rpc.clone();
                async move {
                    tracing::trace!(msg= "Reply with", reply.rpc = ?rpc, reply.peer = %to);
                    if let Err(e) = out.send(rpc).await {
                        tracing::warn!("{}: Failed to reply to {}: {:?}", self.local_id, to, e);
                    }
                }
            })
            .await
    }

    /// Try to establish an ad-hoc connection to `peer`, and send it `rpc`
    async fn send_adhoc<M: Into<Rpc<Addr, Broadcast>>>(&self, peer: &PeerInfo<Addr>, rpc: M) {
        self.subscribers
            .emit(ProtocolEvent::Control(Control::SendAdhoc {
                to: peer.clone(),
                rpc: rpc.into(),
            }))
            .await
    }

    /// Try to establish a persistent to `peer`
    async fn connect(&self, peer: &PeerInfo<Addr>) {
        self.subscribers
            .emit(ProtocolEvent::Control(Control::Connect {
                to: peer.clone(),
            }))
            .await
    }

    fn local_peer_info(&self) -> PeerInfo<Addr> {
        PeerInfo {
            peer_id: self.local_id,
            advertised_info: self.local_ad.clone(),
            seen_addrs: HashSet::with_capacity(0),
        }
    }
}

impl<Storage, Broadcast, Addr, R, W> Drop for Protocol<Storage, Broadcast, Addr, R, W>
where
    Addr: Clone + PartialEq + Eq + Hash,
{
    fn drop(&mut self) {
        // `Relaxed` is presumably ok here, because all we want is to not wrap
        // around, which `saturating_sub` guarantees
        let r = self.ref_count.fetch_update(
            atomic::Ordering::Relaxed,
            atomic::Ordering::Relaxed,
            |x| Some(x.saturating_sub(1)),
        );
        match r {
            Ok(x) | Err(x) if x == 0 => tracing::trace!("`gossip::Protocol` refcount is zero"),
            _ => {},
        }
    }
}
