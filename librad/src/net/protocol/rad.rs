use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    io,
    iter,
    marker::PhantomData,
    mem,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use futures::{
    channel::mpsc,
    sink::{Sink, SinkExt},
    stream::{StreamExt, TryStreamExt},
};
use futures_codec::{CborCodec, CborCodecError, Framed, FramedRead, FramedWrite};
use log::warn;
use rand::{seq::IteratorRandom, Rng};
use rand_pcg::Pcg64Mcg;
use serde::{Deserialize, Serialize};

use crate::{net::connection::Stream, paths::Paths, peer::PeerId};

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
}

#[async_trait]
pub trait Storage<A>: Clone + Send + Sync {
    // TODO: does this have to be async?
    async fn put(&self, have: A) -> PutResult;
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
    /// The number of hops a [`Membership::ForwardJoin`] should be propageted
    /// before it is inserted into the peer's membership table.
    random_walk_length: usize,
    /// The maximum number of peers to include in a shuffle.
    shuffle_sample_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            random_walk_length: 3,
            shuffle_sample_size: 7,
        }
    }
}

/// Placeholder for a datastructure reoresenting the currently connected-to
/// peers
///
/// The number of entries should be bounded.
#[derive(Clone, Default)]
struct ConnectedPeers<A, S> {
    peers: HashMap<PeerId, S>,
    _marker: PhantomData<A>,
}

impl<A, S> ConnectedPeers<A, S>
where
    S: Sink<A> + Unpin,
{
    fn new() -> Self {
        Self {
            peers: HashMap::default(),
            _marker: PhantomData,
        }
    }

    fn insert(&mut self, peer_id: &PeerId, sink: S) {
        if let Some(mut old) = self.peers.insert(peer_id.clone(), sink) {
            let _ = old.close();
        }
    }

    fn remove(&mut self, peer_id: &PeerId) {
        if let Some(mut old) = self.peers.remove(peer_id) {
            let _ = old.close();
        }
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
struct KnownPeers(HashMap<PeerId, PeerInfo>);

impl KnownPeers {
    fn new() -> Self {
        Self(HashMap::default())
    }

    fn insert<I: IntoIterator<Item = PeerInfo>>(&mut self, peers: I) {
        for info in peers {
            let entry = self
                .0
                .entry(info.peer_id.clone())
                .or_insert_with(|| info.clone());
            entry.seen_addrs = entry.seen_addrs.union(&info.seen_addrs).cloned().collect();
        }
    }

    fn sample<R: Rng>(&mut self, rng: &mut R, n: usize) -> Vec<PeerInfo> {
        self.0.values().cloned().choose_multiple(rng, n)
    }
}

// TODO: generalise over `Stream` / `quinn::SendStream` (ie. `AsyncRead +
// AsyncWrite`)
pub type NegotiatedStream<A> = Framed<Stream, CborCodec<Rpc<A>, Rpc<A>>>;
type SendStream<A> = FramedWrite<quinn::SendStream, CborCodec<Rpc<A>, Rpc<A>>>;

#[derive(Clone)]
pub struct Protocol<A: Eq + Hash, S> {
    local_id: PeerId,
    local_ad: PeerAdvertisement,
    paths: Paths,

    config: Config,

    prng: Pcg64Mcg,

    storage: S,
    event_subscribers: Arc<Mutex<Vec<mpsc::UnboundedSender<ProtocolEvent<A>>>>>,

    // TODO: parametrise over SendStream
    connected_peers: Arc<Mutex<ConnectedPeers<Rpc<A>, SendStream<A>>>>,
    known_peers: Arc<Mutex<KnownPeers>>,
}

// TODO(kim): initiate periodic shuffle
impl<A, S> Protocol<A, S>
where
    for<'de> A: Clone + Eq + Hash + Deserialize<'de> + Serialize + 'static,
    S: Storage<A>,
{
    pub fn new(local_id: &PeerId, local_ad: PeerAdvertisement, paths: &Paths, storage: S) -> Self {
        Self {
            local_id: local_id.clone(),
            local_ad,
            paths: paths.clone(),

            config: Config::default(),

            prng: Pcg64Mcg::new(rand::random()),

            storage,
            event_subscribers: Arc::new(Mutex::new(Vec::with_capacity(1))),

            connected_peers: Arc::new(Mutex::new(ConnectedPeers::new())),
            known_peers: Arc::new(Mutex::new(KnownPeers::new())),
        }
    }

    pub async fn announce(&self, have: A) {
        self.broadcast(Gossip::Have(have), None).await
    }

    pub async fn query(&self, want: A) {
        self.broadcast(Gossip::Want(want), None).await
    }

    pub(super) fn subscribe(&self) -> impl futures::Stream<Item = ProtocolEvent<A>> {
        let (tx, rx) = mpsc::unbounded();
        self.event_subscribers.lock().unwrap().push(tx);
        rx
    }

    pub(super) async fn outgoing(
        &self,
        mut stream: NegotiatedStream<A>,
        hello: impl Into<Option<Rpc<A>>>,
    ) -> Result<(), Error> {
        let remote_id = stream.peer_id().clone();
        // This should not be possible, as we prevent it in the TLS handshake.
        // Leaving it here regardless as a sanity check.
        if remote_id == self.local_id {
            return Err(Error::SelfConnection);
        }

        stream
            .send(
                hello
                    .into()
                    .unwrap_or_else(|| Membership::Join(self.local_ad.clone()).into()),
            )
            .await?;

        self.incoming(stream).await
    }

    pub(super) async fn incoming(&self, stream: NegotiatedStream<A>) -> Result<(), Error> {
        use Gossip::*;
        use Membership::*;

        let remote_id = stream.peer_id().clone();

        let (conn, mut recv) = {
            let (stream, codec) = stream.release();
            let (conn, recv, send) = stream.into();

            self.connected_peers
                .lock()
                .unwrap()
                .insert(&remote_id, FramedWrite::new(send, codec.clone()));

            (conn, FramedRead::new(recv, codec))
        };

        // This is a closure because QUIC connections can migrate their network
        // address.
        let seen_addr = |listen_port| {
            let mut addr = conn.remote_address();
            addr.set_port(listen_port);
            addr
        };

        let make_peer_info = |ad: PeerAdvertisement| {
            let seen_as = seen_addr(ad.listen_port);
            PeerInfo {
                peer_id: remote_id.clone(),
                advertised_info: ad,
                seen_addrs: iter::once(seen_as).collect(),
            }
        };

        while let Some(rpc) = recv.try_next().await? {
            match rpc {
                Rpc::Membership(msg) => match msg {
                    Join(ad) => {
                        let peer_info = make_peer_info(ad);

                        self.known_peers
                            .lock()
                            .unwrap()
                            .insert(iter::once(peer_info.clone()));

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

                    Neighbour(ad) => self
                        .known_peers
                        .lock()
                        .unwrap()
                        .insert(iter::once(make_peer_info(ad))),

                    Shuffle { origin, peers, ttl } => {
                        // We're supposed to only remember shuffled peers at
                        // the end of the random walk. Do it anyway for now.
                        self.known_peers.lock().unwrap().insert(peers);

                        if ttl > 0 {
                            let sample = self
                                .known_peers
                                .lock()
                                .unwrap()
                                .sample(&mut self.prng.clone(), self.config.shuffle_sample_size);

                            self.dial_and_send(&origin, ShuffleReply { peers: sample })
                                .await
                        }
                    },

                    ShuffleReply { peers } => self.known_peers.lock().unwrap().insert(peers),
                },

                Rpc::Gossip(msg) => match msg {
                    Have(val) => {
                        // If `val` was new to us, forward to others as it may
                        // be new to them, too. Otherwise, terminate the flood
                        // here.
                        if let PutResult::Applied = self.storage.put(val.clone()).await {
                            self.broadcast(Have(val), &remote_id).await
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

        self.connected_peers.lock().unwrap().remove(&remote_id);

        Ok(())
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
        self.emit_event(ProtocolEvent::DialAndSend(peer.clone(), rpc.into()))
            .await
    }

    async fn emit_event(&self, evt: ProtocolEvent<A>) {
        let mut subscribers = self.event_subscribers.lock().unwrap();

        // Gawd, why is there no `retain` on streams?
        let subscribers1: Vec<_> = futures::stream::iter(subscribers.iter_mut())
            .filter_map(|ch| {
                let evt = evt.clone();
                async move {
                    if ch.send(evt).await.is_err() {
                        Some(ch.clone())
                    } else {
                        None
                    }
                }
            })
            .collect()
            .await;

        mem::replace(&mut *subscribers, subscribers1);
    }
}
