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

//! Main protocol dispatch loop

use std::{
    borrow::Cow,
    collections::HashMap,
    fmt::Debug,
    future::Future,
    io,
    iter,
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::{
        atomic::{self, AtomicUsize},
        Arc,
    },
    task::{Context, Poll},
};

use futures::{
    future::{self, BoxFuture, FutureExt, TryFutureExt},
    lock::Mutex,
    stream::{BoxStream, StreamExt, TryStreamExt},
};
use minicbor::{Decode, Encode};
use thiserror::Error;
use tracing_futures::Instrument;

use crate::{
    git::p2p::{
        server::GitServer,
        transport::{GitStream, GitStreamFactory},
    },
    internal::channel::Fanout,
    net::{
        codec::CborCodecError,
        connection::{CloseReason, LocalInfo, RemoteInfo, Stream},
        gossip,
        quic,
        upgrade::{self, upgrade, with_upgraded, SomeUpgraded, UpgradeRequest, Upgraded},
    },
    peer::PeerId,
};

#[derive(Clone, Debug)]
pub enum ProtocolEvent<A> {
    Connected(PeerId),
    Disconnecting(PeerId),
    Listening(SocketAddr),
    Gossip(gossip::Info<IpAddr, A>),
    Membership(gossip::MembershipInfo<IpAddr>),
}

/// Unification of the different inputs the run loop processes.
///
/// We do this instead of a hand-rolled `Future` so we can use
/// `StreamExt::try_for_each_concurrent`, which allows us to retain control over
/// spawned tasks.
enum Run<'a, A> {
    Discovered {
        peer: PeerId,
        addrs: Vec<SocketAddr>,
    },

    Incoming {
        conn: quic::Connection,
        incoming: BoxStream<'a, quic::Result<quic::Stream>>,
    },

    Gossip {
        event: gossip::ProtocolEvent<IpAddr, A>,
    },
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("no connection to {0}")]
    NoConnection(PeerId),

    #[error(transparent)]
    Upgrade(#[from] upgrade::ErrorSource),

    #[error(transparent)]
    Cbor(#[from] CborCodecError),

    #[error("error handling gossip upgrade")]
    Gossip(#[from] gossip::error::Error),

    #[error("error handling git upgrade")]
    Git(#[source] io::Error),

    #[error(transparent)]
    Quic(#[from] quic::Error),

    #[error(transparent)]
    Io(#[from] io::Error),
}

pub type RunLoop = BoxFuture<'static, ()>;

pub struct Protocol<S, A> {
    gossip: gossip::Protocol<S, A, IpAddr, quic::RecvStream, quic::SendStream>,
    git: GitServer,

    endpoint: quic::Endpoint,

    connections: Arc<Mutex<HashMap<PeerId, quic::Connection>>>,
    subscribers: Fanout<ProtocolEvent<A>>,

    ref_count: Arc<AtomicUsize>,
}

impl<S, A> Clone for Protocol<S, A>
where
    S: Clone,
    A: Clone,
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
            gossip: self.gossip.clone(),
            git: self.git.clone(),
            endpoint: self.endpoint.clone(),
            connections: self.connections.clone(),
            subscribers: self.subscribers.clone(),
            ref_count: self.ref_count.clone(),
        }
    }
}

impl<S, A> Drop for Protocol<S, A> {
    fn drop(&mut self) {
        // `Relaxed` is presumably ok here, because all we want is to not wrap
        // around, which `saturating_sub` guarantees
        let r = self.ref_count.fetch_update(
            atomic::Ordering::Relaxed,
            atomic::Ordering::Relaxed,
            |x| Some(x.saturating_sub(1)),
        );
        match r {
            Ok(x) | Err(x) if x == 0 => {
                tracing::trace!("`net::Protocol` refcount is zero");
                self.endpoint.shutdown()
            },
            _ => {},
        }
    }
}

impl<S, A> Protocol<S, A>
where
    S: gossip::LocalStorage<Update = A> + 'static,
    for<'de> A: Encode + Decode<'de> + Clone + Debug + Send + Sync + 'static,
{
    pub fn new<Disco>(
        gossip: gossip::Protocol<S, A, IpAddr, quic::RecvStream, quic::SendStream>,
        git: GitServer,
        quic::BoundEndpoint { endpoint, incoming }: quic::BoundEndpoint<'static>,
        disco: Disco,
    ) -> (Self, RunLoop)
    where
        Disco: futures::stream::Stream<Item = (PeerId, Vec<SocketAddr>)> + Send + 'static,
    {
        let this = Self {
            gossip,
            git,
            endpoint: endpoint.clone(),
            connections: Arc::new(Mutex::new(HashMap::default())),
            subscribers: Fanout::new(),
            ref_count: Arc::new(AtomicUsize::new(0)),
        };

        let run_loop = {
            let that = this.clone();
            let local_addr = endpoint
                .local_addr()
                .expect("unable to get local endpoint addr");

            let span = tracing::info_span!(
                "Protocol::run",
                local.id = %that.peer_id(),
                local.addr = %local_addr
            );

            // Future which resolves once our refcount reaches zero.
            //
            // This ensures spawned tasks handling ingress/egress streams
            // terminate when the last reference to `this` is dropped.
            struct Bomb {
                /// `ref_count` of `this`
                ref_count: Arc<AtomicUsize>,
            };

            impl Future for Bomb {
                type Output = ();

                fn poll(self: Pin<&mut Self>, _: &mut Context) -> Poll<Self::Output> {
                    if self.ref_count.load(atomic::Ordering::Relaxed) < 1 {
                        Poll::Ready(())
                    } else {
                        Poll::Pending
                    }
                }
            }

            async move {
                let incoming = incoming.map(|(conn, i)| Run::Incoming { conn, incoming: i });
                let bootstrap = disco.map(|(peer, addrs)| Run::Discovered { peer, addrs });
                let gossip_events = that
                    .gossip
                    .subscribe()
                    .await
                    .map(|event| Run::Gossip { event });

                tracing::info!("Listening");
                that.subscribers
                    .emit(ProtocolEvent::Listening(local_addr))
                    .await;

                let eval = futures::stream::select(
                    incoming,
                    futures::stream::select(bootstrap, gossip_events),
                )
                .for_each_concurrent(None, |run| that.eval_run(run));

                future::select(
                    eval.boxed(),
                    Bomb {
                        ref_count: that.ref_count.clone(),
                    },
                )
                .map(|_| ())
                .await;
            }
            .instrument(span)
        };

        (this, Box::pin(run_loop))
    }

    pub fn peer_id(&self) -> PeerId {
        self.gossip.peer_id()
    }

    /// Subscribe to an infinite stream of [`ProtocolEvent`]s.
    ///
    /// The consumer must keep polling the stream, or drop it to cancel the
    /// subscription.
    pub async fn subscribe(&self) -> impl futures::Stream<Item = ProtocolEvent<A>> {
        self.subscribers.subscribe().await
    }

    /// Announce an update to the network
    pub async fn announce(&self, have: A) {
        self.gossip.announce(have).await
    }

    /// Mapping of currently connected [`PeerId`]s and their remote
    /// [`SocketAddr`]s.
    pub async fn connected_peers(&self) -> HashMap<PeerId, SocketAddr> {
        self.connections
            .lock()
            .await
            .iter()
            .map(|(peer_id, conn)| (*peer_id, conn.remote_addr()))
            .collect()
    }

    /// Returns `true` if there is at least one active connection.
    pub async fn has_connections(&self) -> bool {
        !self.connections.lock().await.is_empty()
    }

    /// Returns the number of currently active connections.
    pub async fn num_connections(&self) -> usize {
        self.connections.lock().await.len()
    }

    /// Query the network for an update
    ///
    /// Answers from the network will be available as `ProtocolEvent::Gossip`
    /// when `subscribe`d.
    ///
    /// Note that responses will also cause [`gossip::LocalStorage::put`] to be
    /// invoked, i.e. the local storage will be converged towards the
    /// requested state.
    pub async fn query(&self, want: A) {
        self.gossip.query(want).await
    }

    /// Open a QUIC stream which is upgraded to expect the git protocol
    ///
    /// If no connection to the given peer is currently active, `addr_hints`
    /// will be used to attempt to establish a new connection. `addr_hints` must
    /// be finite -- we use the [`IntoIterator`] trait bound for notational
    /// convenience (one can pass [`None`], for example).
    pub async fn open_git<Addrs>(
        &self,
        to: PeerId,
        addr_hints: Addrs,
    ) -> Result<Upgraded<upgrade::Git, quic::Stream>, Error>
    where
        Addrs: IntoIterator<Item = SocketAddr>,
    {
        self.open_stream(to, upgrade::Git)
            .or_else(|e| async move {
                match e {
                    Error::NoConnection(_) => {
                        // Ensure `addr_hints` has the same lifetime as `incoming`
                        let addr_hints = addr_hints.into_iter().collect::<Vec<_>>();
                        let (conn, incoming) = connect(&self.endpoint, to, addr_hints)
                            .await
                            .ok_or_else(|| Error::NoConnection(to))?;

                        let stream = conn.open_stream().await?;
                        upgrade(stream, upgrade::Git)
                            .await
                            .map_err(|upgrade::Error { stream, source }| {
                                stream.close(CloseReason::InvalidUpgrade);
                                Error::from(source)
                            })
                            .map(|upgraded| {
                                let this = self.clone();
                                tokio::spawn(async move {
                                    this.handle_connect(conn, incoming.boxed(), None).await
                                });
                                upgraded
                            })
                    },
                    _ => Err(e),
                }
            })
            .await
    }

    async fn eval_run(&self, run: Run<'_, A>) {
        match run {
            Run::Discovered { peer, addrs } => {
                tracing::trace!(
                    remote.id = %peer,
                    remote.addrs = ?addrs,
                    "Run::Discovered",
                );

                if !self.connections.lock().await.contains_key(&peer) {
                    if let Some((conn, incoming)) = connect(&self.endpoint, peer, addrs).await {
                        self.handle_connect(conn, incoming.boxed(), None).await;
                    }
                }
            },

            Run::Incoming { conn, incoming } => {
                tracing::trace!(
                    remote.id = %conn.remote_peer_id(),
                    remote.addrs = %conn.remote_addr(),
                    "Run::Incoming",
                );

                if let Err(e) = self.handle_incoming(conn, incoming).await {
                    tracing::warn!("Error processing incoming connections: {}", e)
                }
            },

            Run::Gossip { event } => match event {
                gossip::ProtocolEvent::Control(ctrl) => match ctrl {
                    gossip::Control::SendAdhoc { to, rpc } => {
                        tracing::trace!(remote.id = %to.peer_id, "Run::Rad(SendAdhoc)");

                        let conn = match self.connections.lock().await.get(&to.peer_id) {
                            Some(conn) => Some(conn.clone()),
                            None => connect_peer_info(&self.endpoint, to)
                                .await
                                .map(|(conn, _)| conn),
                        };

                        if let Some(conn) = conn {
                            if let Err(e) = conn
                                .open_stream()
                                .map_err(Error::from)
                                .and_then(|stream| self.outgoing(stream, rpc))
                                .await
                            {
                                tracing::warn!(
                                    "Error handling ad-hoc outgoing stream to {}: {}",
                                    conn.remote_addr(),
                                    e
                                )
                            }
                        }
                    },

                    gossip::Control::Connect { to } => {
                        tracing::trace!(remote.id = %to.peer_id, "Run::Rad(Connect)");

                        if !self.connections.lock().await.contains_key(&to.peer_id) {
                            let conn = connect_peer_info(&self.endpoint, to).await;
                            if let Some((conn, incoming)) = conn {
                                self.handle_connect(conn, incoming.boxed(), None).await
                            }
                        }
                    },

                    gossip::Control::Disconnect(peer) => {
                        tracing::trace!(peer.id = %peer, "Run::Rad(Disconnect)");

                        self.handle_disconnect(peer).await;
                    },
                },

                gossip::ProtocolEvent::Info(info) => {
                    self.subscribers.emit(ProtocolEvent::Gossip(info)).await
                },

                gossip::ProtocolEvent::Membership(info) => {
                    self.subscribers.emit(ProtocolEvent::Membership(info)).await
                },
            },
        }
    }

    async fn handle_connect(
        &self,
        conn: quic::Connection,
        mut incoming: BoxStream<'_, quic::Result<quic::Stream>>,
        hello: impl Into<Option<gossip::Rpc<IpAddr, A>>>,
    ) {
        let remote_id = conn.remote_peer_id();
        tracing::info!(remote.id = %remote_id, "New outgoing connection");

        {
            self.connections
                .lock()
                .await
                .insert(remote_id, conn.clone());
            self.subscribers
                .emit(ProtocolEvent::Connected(remote_id))
                .await;
        }

        let res = futures::try_join!(
            async {
                let outgoing = conn.open_stream().await?;
                self.outgoing(outgoing, hello).await
            },
            async {
                while let Some(stream) = incoming.try_next().await? {
                    self.incoming(stream).await?
                }

                Ok(())
            }
        );

        if let Err(e) = res {
            tracing::warn!("Closing connection with {}, because: {}", remote_id, e);
            conn.close(CloseReason::InternalError);
        };

        self.handle_disconnect(remote_id).await;
    }

    async fn handle_disconnect(&self, peer: PeerId) {
        if let Some(conn) = self.connections.lock().await.remove(&peer) {
            tracing::info!(msg = "Disconnecting", remote.addr = %conn.remote_addr());
            self.subscribers
                .emit(ProtocolEvent::Disconnecting(peer))
                .await
        }
    }

    async fn handle_incoming<Incoming>(
        &self,
        conn: quic::Connection,
        incoming: Incoming,
    ) -> Result<(), Error>
    where
        Incoming: futures::Stream<Item = quic::Result<quic::Stream>> + Unpin,
    {
        let remote_id = conn.remote_peer_id();
        tracing::info!(remote.id = %remote_id, "New incoming connection");

        {
            self.connections.lock().await.insert(remote_id, conn);
            self.subscribers
                .emit(ProtocolEvent::Connected(remote_id))
                .await;
        }

        let res = self.handle_incoming_streams(incoming).await;

        self.handle_disconnect(remote_id).await;

        res
    }

    async fn handle_incoming_streams<Incoming>(&self, mut incoming: Incoming) -> Result<(), Error>
    where
        Incoming: futures::Stream<Item = quic::Result<quic::Stream>> + Unpin,
    {
        while let Some(stream) = incoming.try_next().await? {
            tracing::trace!("New incoming stream");
            let this = self.clone();
            tokio::spawn(async move {
                if let Err(e) = this.incoming(stream).await {
                    tracing::warn!("Incoming stream error: {}", e);
                }
            });
        }

        Ok(())
    }

    async fn outgoing(
        &self,
        stream: quic::Stream,
        hello: impl Into<Option<gossip::Rpc<IpAddr, A>>>,
    ) -> Result<(), Error> {
        match upgrade(stream, upgrade::Gossip).await {
            Err(upgrade::Error { stream, source }) => {
                stream.close(CloseReason::InvalidUpgrade);
                Err(Error::from(source))
            },

            Ok(upgraded) => self
                .gossip
                .outgoing(upgraded, hello)
                .await
                .map_err(|e| e.into()),
        }
    }

    async fn incoming(&self, stream: quic::Stream) -> Result<(), Error> {
        match with_upgraded(stream).await {
            Err(upgrade::Error { stream, source }) => {
                stream.close(CloseReason::InvalidUpgrade);
                Err(Error::from(source))
            },

            Ok(upgraded) => match upgraded {
                SomeUpgraded::Gossip(upgraded) => {
                    self.gossip.incoming(upgraded).await.map_err(Error::Gossip)
                },

                SomeUpgraded::Git(upgraded) => self
                    .git
                    .invoke_service(upgraded.into_stream().split())
                    .await
                    .map_err(Error::Git),
            },
        }
    }

    async fn open_stream<U>(&self, to: PeerId, up: U) -> Result<Upgraded<U, quic::Stream>, Error>
    where
        U: Into<UpgradeRequest>,
    {
        let stream = match self.connections.lock().await.get(&to) {
            Some(conn) => conn.open_stream().await.map_err(Error::from),
            None => Err(Error::NoConnection(to)),
        }?;

        upgrade(stream, up)
            .await
            .map_err(|upgrade::Error { stream, source }| {
                stream.close(CloseReason::InvalidUpgrade);
                Error::from(source)
            })
    }
}

#[async_trait]
impl<S, A> GitStreamFactory for Protocol<S, A>
where
    S: gossip::LocalStorage<Update = A> + 'static,
    for<'de> A: Encode + Decode<'de> + Clone + Debug + Send + Sync + 'static,
{
    async fn open_stream(
        &self,
        to: &PeerId,
        addr_hints: &[SocketAddr],
    ) -> Option<Box<dyn GitStream>> {
        let span = tracing::trace_span!("GitStreamFactory::open_stream", peer.id = %to);

        match self
            .open_git(*to, addr_hints.iter().copied())
            .instrument(span.clone())
            .await
        {
            Ok(s) => Some(Box::new(s)),
            Err(e) => {
                span.in_scope(|| tracing::warn!("Error opening git stream: {}", e));
                None
            },
        }
    }
}

impl<'a, S, A> From<&'a Protocol<S, A>> for Cow<'a, Protocol<S, A>>
where
    S: Clone,
    A: Clone,
{
    fn from(p: &'a Protocol<S, A>) -> Self {
        Cow::Borrowed(p)
    }
}

async fn connect_peer_info(
    endpoint: &quic::Endpoint,
    peer_info: gossip::PeerInfo<IpAddr>,
) -> Option<(
    quic::Connection,
    impl futures::Stream<Item = quic::Result<quic::Stream>> + Unpin,
)> {
    let advertised_port = peer_info.advertised_info.listen_port;
    let addrs = iter::once(peer_info.advertised_info.listen_addr)
        .chain(peer_info.seen_addrs)
        .map(move |ip| SocketAddr::new(ip, advertised_port));
    connect(endpoint, peer_info.peer_id, addrs).await
}

async fn connect<Addrs>(
    endpoint: &quic::Endpoint,
    peer_id: PeerId,
    addrs: Addrs,
) -> Option<(
    quic::Connection,
    impl futures::Stream<Item = quic::Result<quic::Stream>> + Unpin,
)>
where
    Addrs: IntoIterator<Item = SocketAddr>,
{
    futures::stream::iter(addrs)
        .filter_map(|addr| {
            let mut endpoint = endpoint.clone();
            tracing::info!(remote.id = %peer_id, "Establishing connection");
            Box::pin(async move {
                match endpoint.connect(peer_id, &addr).await {
                    Ok(conn) => Some(conn),
                    Err(e) => {
                        tracing::warn!("Could not connect to {} at {}: {}", peer_id, addr, e);
                        None
                    },
                }
            })
        })
        .next()
        .await
}
