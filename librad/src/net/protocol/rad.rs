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
        Mutex,
    },
    thread,
    time::Duration,
};

use async_trait::async_trait;
use futures::{
    executor::block_on,
    sink::{Sink, SinkExt},
    stream::{StreamExt, TryStreamExt},
};
use futures_codec::{CborCodec, CborCodecError, Framed, FramedRead, FramedWrite};
use futures_timer::Delay;
use log::warn;
use rand::{seq::IteratorRandom, Rng};
use rand_pcg::Pcg64Mcg;
use serde::{Deserialize, Serialize};

use crate::{
    internal::channel::Fanout,
    net::connection::{SendStream, Stream},
    paths::Paths,
    peer::PeerId,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Rpc<A> {
    Membership(Membership),
    Gossip(Gossip<A>),
}

impl<A> From<Membership> for Rpc<A> {
    fn from(m: Membership) -> Self {
        Self::Membership(m)
    }
}

impl<A> From<Gossip<A>> for Rpc<A> {
    fn from(g: Gossip<A>) -> Self {
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
pub enum Gossip<A> {
    Have(A),
    Want(A),
}

pub enum PutResult {
    Applied,
    Stale,
    Error,
}

// TODO: does this have to be async?
#[async_trait]
pub trait LocalStorage<A>: Clone + Send + Sync {
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
    async fn put(&self, provider: &PeerId, has: A) -> PutResult;

    /// Ask the local storage is value `A` is available.
    ///
    /// This is used to notify the asking peer that they may fetch value `A`
    /// from us.
    async fn ask(&self, want: &A) -> bool;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerInfo {
    pub peer_id: PeerId,
    pub advertised_info: PeerAdvertisement,
    pub seen_addrs: HashSet<SocketAddr>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerAdvertisement {
    pub listen_port: u16,
    pub capabilities: HashSet<Capability>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum Capability {
    Reserved = 0,
}

#[derive(Clone)]
pub enum ProtocolEvent<A> {
    DialAndSend(PeerInfo, Rpc<A>),
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
struct Config {
    /// Maximum number of active connections.
    max_active: usize,
    /// The number of hops a [`Membership::ForwardJoin`] or
    /// [`Membership::Shuffle`] should be propageted.
    random_walk_length: usize,
    /// The maximum number of peers to include in a shuffle.
    shuffle_sample_size: usize,
    /// Interval in which to perform a shuffle.
    shuffle_interval: Duration,
    /// Interval in which to attempt to promote a passive peer.
    promote_interval: Duration,
}

impl Default for Config {
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
                let _ = ejected.close();
            });
            Some(eject)
        } else {
            self.peers.insert(peer_id.clone(), sink).map(|mut old| {
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
pub type NegotiatedStream<A> = Framed<Stream, CborCodec<Rpc<A>, Rpc<A>>>;
type NegotiatedSendStream<A> = FramedWrite<SendStream, CborCodec<Rpc<A>, Rpc<A>>>;
type ConnectedPeersImpl<A> = ConnectedPeers<Rpc<A>, NegotiatedSendStream<A>, Pcg64Mcg>;

#[derive(Clone)]
pub struct Protocol<A, S> {
    local_id: PeerId,
    local_ad: PeerAdvertisement,
    paths: Paths,

    config: Config,

    prng: Pcg64Mcg,

    storage: S,
    subscribers: Fanout<ProtocolEvent<A>>,

    connected_peers: Arc<Mutex<ConnectedPeersImpl<A>>>,
    known_peers: Arc<Mutex<KnownPeers<Pcg64Mcg>>>,

    dropped: Arc<AtomicBool>,
}

// TODO(kim): initiate periodic shuffle
impl<A, S> Protocol<A, S>
where
    for<'de> A: Clone + Eq + Send + Hash + Deserialize<'de> + Serialize + 'static,
{
    pub fn new(local_id: &PeerId, local_ad: PeerAdvertisement, paths: &Paths, storage: S) -> Self
    where
        S: LocalStorage<A> + 'static,
    {
        let prng = Pcg64Mcg::new(rand::random());
        let config = Config::default();
        let connected_peers = Arc::new(Mutex::new(ConnectedPeers::new(
            config.max_active,
            prng.clone(),
        )));
        let known_peers = Arc::new(Mutex::new(KnownPeers::new(prng.clone())));

        let this = Self {
            local_id: local_id.clone(),
            local_ad,
            paths: paths.clone(),

            config,
            prng,
            storage,

            subscribers: Fanout::new(),

            connected_peers,
            known_peers,

            dropped: Arc::new(AtomicBool::new(false)),
        };

        let this1 = this.clone();
        thread::spawn(move || block_on(this1.run_periodic_tasks()));

        this
    }

    pub async fn announce(&self, have: A) {
        self.broadcast(Gossip::Have(have), None).await
    }

    pub async fn query(&self, want: A) {
        self.broadcast(Gossip::Want(want), None).await
    }

    pub(super) fn subscribe(&self) -> impl futures::Stream<Item = ProtocolEvent<A>> {
        self.subscribers.subscribe()
    }

    pub(super) async fn outgoing(
        &self,
        mut stream: NegotiatedStream<A>,
        hello: impl Into<Option<Rpc<A>>>,
    ) -> Result<(), Error>
    where
        S: LocalStorage<A>,
    {
        let remote_id = stream.peer_id().clone();
        // This should not be possible, as we prevent it in the TLS handshake.
        // Leaving it here regardless as a sanity check.
        if remote_id == self.local_id {
            return Err(Error::SelfConnection);
        }

        let hello = hello
            .into()
            .unwrap_or_else(|| Membership::Join(self.local_ad.clone()).into());
        stream.send(hello).await?;

        self.incoming(stream).await
    }

    pub(super) async fn incoming(&self, stream: NegotiatedStream<A>) -> Result<(), Error>
    where
        S: LocalStorage<A>,
    {
        use Gossip::*;
        use Membership::*;

        let remote_id = stream.peer_id().clone();

        // This is a bit of a hack: in order to keep track of the connected
        // peers, and to be able to broadcast messages to them, we need to move
        // out the send stream again. Ie. we loop over the recv stream here, and
        // use `ConnectedPeers` when we want to send something.
        let mut recv = {
            let (stream, codec) = stream.release();
            let (send, recv) = stream.split();

            if let Some(ejected) =
                self.add_connected(&remote_id, FramedWrite::new(send, codec.clone()))
            {
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

        let make_peer_info = |ad: PeerAdvertisement, addr: SocketAddr| {
            // Remember both the advertised and the actually seen port
            let mut addr1 = addr; // `SocketAddr` is `Copy`
            addr1.set_port(ad.listen_port);

            PeerInfo {
                peer_id: remote_id.clone(),
                advertised_info: ad,
                seen_addrs: vec![addr1, addr].into_iter().collect(),
            }
        };

        while let Some(rpc) = recv.try_next().await? {
            match rpc {
                Rpc::Membership(msg) => match msg {
                    Join(ad) => {
                        let peer_info = make_peer_info(ad, recv.remote_address());

                        self.add_known(iter::once(peer_info.clone()));
                        self.broadcast(
                            ForwardJoin {
                                joined: peer_info,
                                ttl: self.config.random_walk_length,
                            },
                            &remote_id,
                        )
                        .await
                    },

                    ForwardJoin { joined, ttl } => {
                        if ttl == 0 {
                            self.dial_and_send(&joined, Neighbour(self.local_ad.clone()))
                                .await
                        } else {
                            self.broadcast(
                                ForwardJoin {
                                    joined,
                                    ttl: ttl - 1,
                                },
                                &remote_id,
                            )
                            .await
                        }
                    },

                    Neighbour(ad) => {
                        self.add_known(iter::once(make_peer_info(ad, recv.remote_address())))
                    },

                    Shuffle { origin, peers, ttl } => {
                        // We're supposed to only remember shuffled peers at
                        // the end of the random walk. Do it anyway for now.
                        self.add_known(peers);

                        if ttl > 0 {
                            let sample = self.sample_known();
                            self.dial_and_send(&origin, ShuffleReply { peers: sample })
                                .await
                        }
                    },

                    ShuffleReply { peers } => self.add_known(peers),
                },

                Rpc::Gossip(msg) => match msg {
                    Have(val) => {
                        match self.storage.put(&remote_id, val.clone()).await {
                            // `val` was new, and is now fetched to local
                            // storage. Let connected peers know they can now
                            // fetch it from us.
                            PutResult::Applied => self.broadcast(Have(val), &remote_id).await,
                            // Meh. Request retransmission.
                            // TODO: actually... we may only want to ask the
                            // peer we got the `Have` from in the first place.
                            // But what if that went away in the meantime?
                            PutResult::Error => self.broadcast(Want(val), None).await,
                            // We are up-to-date, don't do anything
                            PutResult::Stale => {},
                        }
                    },
                    Want(val) => {
                        if self.storage.ask(&val).await {
                            self.reply(&remote_id, Have(val)).await
                        } else {
                            self.broadcast(Want(val), &remote_id).await
                        }
                    },
                },
            }
        }

        self.remove_connected(&remote_id);

        Ok(())
    }

    fn add_connected(&self, peer_id: &PeerId, out: NegotiatedSendStream<A>) -> Option<PeerId> {
        self.connected_peers.lock().unwrap().insert(peer_id, out)
    }

    fn remove_connected(&self, peer_id: &PeerId) {
        self.connected_peers.lock().unwrap().remove(peer_id)
    }

    fn add_known<I: IntoIterator<Item = PeerInfo>>(&self, peers: I) {
        self.known_peers.lock().unwrap().insert(peers)
    }

    fn sample_known(&self) -> Vec<PeerInfo> {
        self.known_peers
            .lock()
            .unwrap()
            .sample(self.config.shuffle_sample_size)
    }

    async fn run_periodic_tasks(&self) {
        loop {
            if self.dropped.load(atomic::Ordering::Relaxed) {
                break;
            }

            let shuffle = async {
                Delay::new(self.config.shuffle_interval).await;
                self.shuffle().await;
            };

            let promote = async {
                Delay::new(self.config.promote_interval).await;
                self.promote_random().await;
            };

            // FIXME(kim): I would think we actually want `futures::select!`,
            // in order to break already when the quicker one resolves. I don't
            // get the semantics of the `FusedFuture` + `Unpin` requirements,
            // tho.
            futures::join!(shuffle, promote);
        }
    }

    async fn shuffle(&self) {
        let mut connected = self.connected_peers.lock().unwrap();
        if let Some((recipient, recipient_send)) = connected.random() {
            // Note: we should pick from the connected peers first, padding with
            // passive ones up to `shuffle_sample_size`. However, we don't track
            // the advertised info for those, as it will be available only later
            // (if and when they send it to us). Since in the latter case we
            // _will_ insert into `known_peers`, it doesn't really matter. The
            // `KnownPeers` type should make a weighted random choice
            // eventually.
            let sample = self.sample_known();
            recipient_send
                .send(
                    Membership::Shuffle {
                        origin: PeerInfo {
                            peer_id: self.local_id.clone(),
                            advertised_info: self.local_ad.clone(),
                            // We don't know our public addresses
                            seen_addrs: HashSet::with_capacity(0),
                        },
                        peers: sample,
                        ttl: self.config.random_walk_length,
                    }
                    .into(),
                )
                .await
                .unwrap_or_else(|e| warn!("Failed to send shuffle to {}: {:?}", recipient, e))
        }
    }

    async fn promote_random(&self) {
        if let Some(candidate) = self.known_peers.lock().unwrap().random() {
            self.dial_and_send(&candidate, Membership::Neighbour(self.local_ad.clone()))
                .await
        }
    }

    /// Send an [`Rpc`] to all currently connected peers, except `excluding`
    async fn broadcast<'a, R, X>(&self, rpc: R, excluding: X)
    where
        R: Into<Rpc<A>>,
        X: Into<Option<&'a PeerId>>,
    {
        let rpc = rpc.into();
        let excluding = excluding.into();

        let mut connected_peers = self.connected_peers.lock().unwrap();
        futures::stream::iter(
            connected_peers
                .iter_mut()
                .filter(|(peer_id, _)| Some(*peer_id) != excluding),
        )
        .for_each_concurrent(None, |(peer, out)| {
            let rpc = rpc.clone();
            async move {
                // If this returns an error, it is likely the receiving end has
                // stopped working, too. Hence, we don't need to propagate
                // errors here. This statement will need some empirical
                // evidence.
                if let Err(e) = out.send(rpc).await {
                    warn!("Failed to send broadcast message to {}: {:?}", peer, e)
                }
            }
        })
        .await
    }

    async fn reply<R: Into<Rpc<A>>>(&self, to: &PeerId, rpc: R) {
        let rpc = rpc.into();
        futures::stream::iter(self.connected_peers.lock().unwrap().get_mut(to))
            .for_each(|out| {
                let rpc = rpc.clone();
                async move {
                    if let Err(e) = out.send(rpc).await {
                        warn!("Failed to reply to {}: {:?}", to, e);
                    }
                }
            })
            .await
    }

    /// Try to establish an ad-hoc connection to `peer`, and send it `rpc`
    async fn dial_and_send<R: Into<Rpc<A>>>(&self, peer: &PeerInfo, rpc: R) {
        self.subscribers
            .emit(ProtocolEvent::DialAndSend(peer.clone(), rpc.into()))
            .await
    }
}

impl<A, S> Drop for Protocol<A, S> {
    fn drop(&mut self) {
        self.dropped.store(true, atomic::Ordering::Relaxed)
    }
}
