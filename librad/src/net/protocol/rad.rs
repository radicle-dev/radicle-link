use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    io,
    iter,
    marker::PhantomData,
    net::SocketAddr,
    sync::{
        atomic::{self, AtomicBool},
        Arc,
    },
    time::Duration,
};

use futures::{
    channel::mpsc,
    lock::Mutex,
    sink::{Sink, SinkExt},
    stream::{StreamExt, TryStreamExt},
};
use futures_codec::{CborCodec, CborCodecError, Framed, FramedRead, FramedWrite};
use futures_timer::Delay;
use log::{info, trace, warn};
use rand::{seq::IteratorRandom, Rng};
use rand_pcg::Pcg64Mcg;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

use crate::{
    internal::channel::Fanout,
    net::connection::{SendStream, Stream},
    peer::PeerId,
    project::ProjectId,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Rpc {
    Membership(Membership),
    Gossip(Gossip),
}

impl From<Membership> for Rpc {
    fn from(m: Membership) -> Self {
        Self::Membership(m)
    }
}

impl From<Gossip> for Rpc {
    fn from(g: Gossip) -> Self {
        Self::Gossip(g)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Membership {
    Join(PeerAdvertisement),
    ForwardJoin {
        joined: PeerInfo,
        ttl: usize,
    },
    Neighbour(PeerAdvertisement),
    Shuffle {
        origin: PeerInfo,
        peers: Vec<PeerInfo>,
        ttl: usize,
    },
    ShuffleReply {
        peers: Vec<PeerInfo>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Gossip {
    Have { origin: PeerInfo, val: Update },
    Want { origin: PeerInfo, val: Update },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Update {
    Project {
        project: ProjectId,
        head: Option<Ref>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ref {
    name: String,
    target: Vec<u8>,
}

#[derive(Debug)]
pub enum PutResult {
    Applied,
    Stale,
    Uninteresting,
    Error,
}

pub trait LocalStorage: Clone + Send + Sync {
    /// Notify the local storage that a new value is available.
    ///
    /// If the value was stored locally already, [`PutResult::Stale`] must be
    /// returned. Otherwise, [`PutResult::Applied`] indicates that we _now_
    /// have the value locally, and other peers may fetch it from us.
    ///
    /// [`PutResult::Error`] indicates that a storage error occurred -- either
    /// the implementer wasn't able to determine if the local storage is
    /// up-to-date, or it was not possible to fetch the actual state from
    /// the `provider`. In this case, the network is asked to retransmit
    /// [`Gossip::Have`], so we can eventually try again.
    fn put(&self, provider: &PeerId, has: Update) -> PutResult;

    /// Ask the local storage if value `A` is available.
    ///
    /// This is used to notify the asking peer that they may fetch value `A`
    /// from us.
    fn ask(&self, want: &Update) -> bool;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerInfo {
    pub peer_id: PeerId,
    pub advertised_info: PeerAdvertisement,
    pub seen_addrs: HashSet<SocketAddr>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerAdvertisement {
    pub listen_addr: SocketAddr,
    pub capabilities: HashSet<Capability>,
}

impl PeerAdvertisement {
    pub fn new(listen_addr: SocketAddr) -> Self {
        Self {
            listen_addr,
            capabilities: HashSet::default(),
        }
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum Capability {
    Reserved = 0,
}

#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
pub enum ProtocolEvent {
    SendAdhoc(PeerInfo, Rpc),
    Connect(PeerInfo, Rpc),
    Disconnect(PeerId),
}

#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display = "Invalid payload")]
    InvalidPayload(#[fail(cause)] serde_cbor::Error),

    #[fail(display = "Connection to self")]
    SelfConnection,

    #[fail(display = "{}", 0)]
    Io(#[fail(cause)] io::Error),
}

impl From<CborCodecError> for Error {
    fn from(err: CborCodecError) -> Self {
        match err {
            CborCodecError::Cbor(e) => Self::InvalidPayload(e),
            CborCodecError::Io(e) => Self::Io(e),
        }
    }
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
struct ConnectedPeers<A, S, R> {
    max_peers: usize,
    rng: R,
    peers: HashMap<PeerId, S>,
    _marker: PhantomData<A>,
}

impl<A, S, R> ConnectedPeers<A, S, R>
where
    S: Sink<A> + Unpin,
    R: Rng,
{
    fn new(max_peers: usize, rng: R) -> Self {
        Self {
            max_peers,
            rng,
            peers: HashMap::default(),
            _marker: PhantomData,
        }
    }

    fn insert(&mut self, peer_id: &PeerId, sink: S) -> Option<PeerId> {
        if !self.peers.contains_key(&peer_id) && self.peers.len() + 1 > self.max_peers {
            let eject = self
                .peers
                .keys()
                .choose(&mut self.rng)
                .expect("Iterator must contain at least 1 element, as per the if condition. qed")
                .clone();

            self.peers.insert(peer_id.clone(), sink);
            self.peers.remove(&eject).iter_mut().for_each(|ejected| {
                trace!("random peer ejection: {}", eject);
                let _ = ejected.close();
            });
            Some(eject)
        } else {
            self.peers.insert(peer_id.clone(), sink).map(|mut old| {
                trace!("duplicate peer ejection: {}", peer_id);
                let _ = old.close();
                peer_id.clone()
            })
        }
    }

    fn remove(&mut self, peer_id: &PeerId) {
        if let Some(mut old) = self.peers.remove(peer_id) {
            let _ = old.close();
        }
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

// TODO: generalise over `Stream` / `SendStream` (ie. `AsyncRead + AsyncWrite`)
pub type NegotiatedStream = Framed<Stream, CborCodec<Rpc, Rpc>>;
type NegotiatedSendStream = FramedWrite<SendStream, CborCodec<Rpc, Rpc>>;
type ConnectedPeersImpl = ConnectedPeers<Rpc, NegotiatedSendStream, Pcg64Mcg>;

#[derive(Clone)]
pub struct Protocol<S> {
    local_id: PeerId,
    local_ad: PeerAdvertisement,

    mparams: MembershipParams,

    prng: Pcg64Mcg,

    storage: S,
    subscribers: Fanout<ProtocolEvent>,

    connected_peers: Arc<Mutex<ConnectedPeersImpl>>,
    known_peers: Arc<Mutex<KnownPeers<Pcg64Mcg>>>,

    dropped: Arc<AtomicBool>,
}

impl<S> Protocol<S> {
    pub fn new(
        local_id: &PeerId,
        local_ad: PeerAdvertisement,
        mparams: MembershipParams,
        storage: S,
    ) -> Self
    where
        S: LocalStorage + 'static,
    {
        let prng = Pcg64Mcg::new(rand::random());
        let connected_peers = Arc::new(Mutex::new(ConnectedPeers::new(
            mparams.max_active,
            prng.clone(),
        )));
        let known_peers = Arc::new(Mutex::new(KnownPeers::new(prng.clone())));

        let this = Self {
            local_id: local_id.clone(),
            local_ad,

            mparams,
            prng,
            storage,

            subscribers: Fanout::new(),

            connected_peers,
            known_peers,

            dropped: Arc::new(AtomicBool::new(false)),
        };

        let this1 = this.clone();
        tokio::spawn(async move { this1.run_periodic_tasks().await });

        this
    }

    pub async fn announce(&self, have: Update) {
        self.broadcast(
            Gossip::Have {
                origin: self.local_peer_info(),
                val: have,
            },
            None,
        )
        .await
    }

    pub async fn query(&self, want: Update) {
        self.broadcast(
            Gossip::Want {
                origin: self.local_peer_info(),
                val: want,
            },
            None,
        )
        .await
    }

    pub(super) async fn subscribe(&self) -> mpsc::UnboundedReceiver<ProtocolEvent> {
        self.subscribers.subscribe().await
    }

    pub(super) async fn outgoing(
        &self,
        mut stream: NegotiatedStream,
        hello: impl Into<Option<Rpc>>,
    ) -> Result<(), Error>
    where
        S: LocalStorage,
    {
        let remote_id = stream.peer_id().clone();
        trace!("{}: Outgoing to {}", self.local_id, remote_id);
        // This should not be possible, as we prevent it in the TLS handshake.
        // Leaving it here regardless as a sanity check.
        if remote_id == self.local_id {
            return Err(Error::SelfConnection);
        }

        let hello = hello
            .into()
            .unwrap_or_else(|| Membership::Join(self.local_ad.clone()).into());
        trace!("{}: Hello: {:?}", self.local_id, hello);
        stream.send(hello).await?;

        self.incoming(stream).await
    }

    pub(super) async fn incoming(&self, stream: NegotiatedStream) -> Result<(), Error>
    where
        S: LocalStorage,
    {
        let remote_id = stream.peer_id().clone();
        trace!("{}: Incoming from {}", self.local_id, remote_id);

        // This is a bit of a hack: in order to keep track of the connected
        // peers, and to be able to broadcast messages to them, we need to move
        // out the send stream again. Ie. we loop over the recv stream here, and
        // use `ConnectedPeers` when we want to send something.
        let mut recv = {
            let (stream, codec) = stream.release();
            let (recv, send) = stream.split();

            if let Some(ejected) = self
                .add_connected(&remote_id, FramedWrite::new(send, codec.clone()))
                .await
            {
                trace!("{}: Ejecting connected peer {}", self.local_id, ejected);
                // Note: if the ejected peer never sent us a `Join` or
                // `Neighbour`, it isn't behaving well, so we can forget about
                // it here. Otherwise, we should already have it in
                // `known_peers`.
                self.subscribers
                    .emit(ProtocolEvent::Disconnect(ejected))
                    .await
            }

            FramedRead::new(recv, codec)
        };

        loop {
            match recv.try_next().await {
                Ok(Some(rpc)) => match rpc {
                    Rpc::Membership(msg) => {
                        self.handle_membership(&remote_id, recv.remote_address(), msg)
                            .await
                    },
                    Rpc::Gossip(msg) => self.handle_gossip(&remote_id, msg).await,
                },
                Ok(None) => {},
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

    async fn handle_membership(&self, remote_id: &PeerId, remote_addr: SocketAddr, msg: Membership)
    where
        S: LocalStorage,
    {
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
                    "{}: Join with peer_info: \
                                peer_id: {}, \
                                advertised_info: {:?}, \
                                seen_addrs: {:?}",
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
    }

    async fn handle_gossip(&self, remote_id: &PeerId, msg: Gossip)
    where
        S: LocalStorage,
    {
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
                    // TODO: actually... we may only want to ask the peer we got
                    // the `Have` from in the first place.  But what if that
                    // went away in the meantime?
                    PutResult::Error => {
                        /*
                        info!(
                            "{}: Error applying {:?}, requesting retransmission",
                            self.local_id, val
                        );
                        self.broadcast(
                            Want {
                                origin: self.local_peer_info(),
                                val,
                            },
                            None,
                        )
                        .await
                        */
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
    }

    async fn add_connected(&self, peer_id: &PeerId, out: NegotiatedSendStream) -> Option<PeerId> {
        self.connected_peers.lock().await.insert(peer_id, out)
    }

    async fn remove_connected(&self, peer_id: &PeerId) {
        self.connected_peers.lock().await.remove(peer_id)
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

    async fn run_periodic_tasks(&self) {
        loop {
            if self.dropped.load(atomic::Ordering::Relaxed) {
                break;
            }

            let shuffle = async {
                Delay::new(self.mparams.shuffle_interval).await;
                self.shuffle().await;
            };

            let promote = async {
                Delay::new(self.mparams.promote_interval).await;
                self.promote_random().await;
            };

            // FIXME(kim): I would think we actually want `futures::select!`,
            // in order to be able break as soon as the quicker one resolves.
            // I don't get the semantics of the `FusedFuture` + `Unpin`
            // requirements, tho.
            futures::join!(shuffle, promote);
        }
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
    async fn broadcast<'a, R, X>(&self, rpc: R, excluding: X)
    where
        R: Into<Rpc>,
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

    async fn reply<R: Into<Rpc>>(&self, to: &PeerId, rpc: R) {
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
    async fn send_adhoc<R: Into<Rpc>>(&self, peer: &PeerInfo, rpc: R) {
        self.subscribers
            .emit(ProtocolEvent::SendAdhoc(peer.clone(), rpc.into()))
            .await
    }

    /// Try to establish a persistent to `peer` with initial `rpc`
    async fn connect<R: Into<Rpc>>(&self, peer: &PeerInfo, rpc: R) {
        self.subscribers
            .emit(ProtocolEvent::Connect(peer.clone(), rpc.into()))
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

impl<S> Drop for Protocol<S> {
    fn drop(&mut self) {
        self.dropped.store(true, atomic::Ordering::Relaxed)
    }
}
