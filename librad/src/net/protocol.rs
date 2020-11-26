// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

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
        RwLock,
    },
    task::{Context, Poll},
};

use futures::{
    future::{self, BoxFuture, FutureExt, TryFutureExt},
    io::AsyncWrite,
    stream::{StreamExt, TryStreamExt},
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
        connection::{Closable, CloseReason, LocalInfo, RemoteInfo, Stream},
        conntrack,
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
        incoming: quic::IncomingStreams<'a>,
    },

    Gossip {
        event: gossip::ProtocolEvent<IpAddr, A>,
    },
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("no connection to {0}")]
    NoConnection(PeerId),

    #[error("duplicate connection")]
    DuplicateConnection,

    #[error("unsupported upgrade requested")]
    UnsupportedUpgrade,

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

    connections: Arc<RwLock<conntrack::Connections>>,
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
            connections: Arc::new(RwLock::new(conntrack::Connections::default())),
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
    pub fn connected_peers(&self) -> HashMap<PeerId, SocketAddr> {
        self.connections
            .read()
            .unwrap()
            .iter()
            .map(|(peer_id, conn)| (*peer_id, conn.remote_addr()))
            .collect()
    }

    /// Returns `true` if there is at least one active connection.
    pub async fn has_connections(&self) -> bool {
        !self.connections.read().unwrap().is_empty()
    }

    /// Returns the number of currently active connections.
    pub fn num_connections(&self) -> usize {
        self.connections.read().unwrap().len()
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
    ) -> Result<Upgraded<upgrade::Git, quic::BidiStream>, Error>
    where
        Addrs: IntoIterator<Item = SocketAddr>,
    {
        match self.open_bidi(to, upgrade::Git).await {
            Ok(upgraded) => Ok(upgraded),
            Err(e) => match e {
                Error::NoConnection(_) => {
                    // Ensure `addr_hints` has the same lifetime as `incoming`
                    let addr_hints = addr_hints.into_iter().collect::<Vec<_>>();
                    let (conn, incoming) = connect(&self.endpoint, to, addr_hints)
                        .await
                        .ok_or(Error::NoConnection(to))?;

                    let stream = conn.open_bidi().await?;
                    let upgraded = upgrade(stream, upgrade::Git).await.map_err(
                        |upgrade::Error { stream, source }| {
                            stream.close(CloseReason::InvalidUpgrade);
                            Error::from(source)
                        },
                    )?;
                    let this = self.clone();
                    tokio::spawn(async move { this.handle_connect(conn, incoming, None).await });
                    Ok(upgraded)
                },

                _ => Err(e),
            },
        }
    }

    async fn eval_run(&self, run: Run<'_, A>) {
        match run {
            Run::Discovered { peer, addrs } => {
                tracing::trace!(
                    remote.id = %peer,
                    remote.addrs = ?addrs,
                    "Run::Discovered",
                );

                // XXX: needs reconsideration: we leave it to the supplier of the
                // disco stream to initiate a re-connect by yielding an element
                // we actually saw before. The consequence is that existing
                // connections will be terminated.
                if let Some((conn, incoming)) = connect(&self.endpoint, peer, addrs).await {
                    self.handle_connect(conn, incoming, None).await;
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
                        let to_peer = to.peer_id;
                        tracing::trace!(remote.id = %to_peer, "Run::Rad(SendAdhoc)");

                        let conn = {
                            let connections = self.connections.read().unwrap();
                            connections.get(&to_peer).map(Clone::clone)
                        };
                        let conn = match conn {
                            Some(conn) => Some(conn),
                            None =>
                            // TODO: track connection once conntrack is sane
                            {
                                connect_peer_info(&self.endpoint, to)
                                    .await
                                    .map(|(conn, _)| conn)
                            },
                        };

                        match conn {
                            None => tracing::warn!("failed to obtain connection for adhoc rpc"),
                            Some(conn) => async {
                                let stream = conn.open_uni().await.map_err(Error::from)?;
                                let upgraded = upgrade_stream(stream, upgrade::Gossip).await?;
                                self.gossip
                                    .outgoing_uni(upgraded, rpc)
                                    .await
                                    .map_err(Error::from)
                            }
                            .await
                            .unwrap_or_else(|e| {
                                tracing::warn!("error delivering adhoc rpc to {}: {}", to_peer, e)
                            }),
                        }
                    },

                    gossip::Control::Connect { to } => {
                        tracing::trace!(remote.id = %to.peer_id, "Run::Rad(Connect)");

                        let conn = {
                            let connections = self.connections.read().unwrap();
                            connections.get(&to.peer_id).map(Clone::clone)
                        };
                        match conn {
                            None => {
                                let conn = connect_peer_info(&self.endpoint, to).await;
                                if let Some((conn, incoming)) = conn {
                                    self.handle_connect(conn, incoming, None).await
                                }
                            },

                            Some(conn) => {
                                match conn.open_bidi().await {
                                    Ok(stream) => {
                                        // The incoming future should still be
                                        // running, so it's enough to drive an
                                        // outgoing
                                        if let Err(e) = self.outgoing_bidi(stream, None).await {
                                            tracing::warn!("error handling outgoing stream: {}", e);
                                        }
                                    },

                                    Err(e) => {
                                        tracing::warn!("error opening outgoing stream: {}", e);
                                        self.disconnect(conn, CloseReason::ConnectionError).await
                                    },
                                }
                            },
                        }
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

    async fn handle_connect<'a>(
        &'a self,
        conn: quic::Connection,
        incoming: quic::IncomingStreams<'a>,
        hello: impl Into<Option<gossip::Rpc<IpAddr, A>>>,
    ) {
        let remote_id = conn.remote_peer_id();
        tracing::info!(remote.id = %remote_id, "new outgoing connection");

        {
            let mut connections = self.connections.write().unwrap();
            if let Some(prev) = connections.insert(conn.clone()) {
                tracing::warn!(
                    "new outgoing ejects previous connection to {} @ {}",
                    remote_id,
                    prev.remote_addr()
                );
                drop(connections);
                prev.close(CloseReason::DuplicateConnection);
            }
        }

        self.subscribers
            .emit(ProtocolEvent::Connected(remote_id))
            .await;

        // XXX: potential race here: we expect that, if we ejected a previous
        // connection, all stream-processing futures associated with are done by
        // now, and the `CONNECTION_CLOSE` is in the send buffers. There is no
        // way we can assert this, though, so our fresh connection `conn` might
        // be rejected by the other end.

        let res = futures::try_join!(
            async {
                let outgoing = conn.open_bidi().await?;
                self.outgoing_bidi(outgoing, hello).await
            },
            self.handle_incoming_streams(incoming)
        );

        self.disconnect(
            conn,
            res.ok().and(None).or(Some(CloseReason::InternalError)),
        )
        .await
    }

    #[tracing::instrument(
        skip(self, conn, reason),
        fields(
            remote_peer = %conn.remote_peer_id(),
            remote_addr = %conn.remote_addr(),
        )
    )]
    async fn disconnect<R>(&self, conn: quic::Connection, reason: R)
    where
        R: Into<Option<CloseReason>>,
    {
        let remote_peer = conn.remote_peer_id();
        tracing::info!("disconnecting");
        {
            let was_removed = {
                let mut connections = self.connections.write().unwrap();
                connections.remove(&conn)
            };
            if was_removed {
                self.subscribers
                    .emit(ProtocolEvent::Disconnecting(remote_peer))
                    .await
            } else {
                tracing::info!("already gone")
            }
        }
        conn.close(reason.into().unwrap_or(CloseReason::ProtocolDisconnect))
    }

    async fn handle_incoming(
        &self,
        conn: quic::Connection,
        incoming: quic::IncomingStreams<'_>,
    ) -> Result<(), Error> {
        let remote_id = conn.remote_peer_id();
        tracing::info!(remote.id = %remote_id, "new incoming connection");

        if self.connections.read().unwrap().has_connection(&remote_id) {
            tracing::warn!(remote.id = %remote_id, "rejecting duplicate incoming connection");

            self.connections.write().unwrap().remove(&conn);
            drop(incoming);
            conn.close(CloseReason::DuplicateConnection);

            Err(Error::DuplicateConnection)
        } else {
            let _prev = self.connections.write().unwrap().insert(conn.clone());
            debug_assert!(_prev.is_none());

            self.subscribers
                .emit(ProtocolEvent::Connected(remote_id))
                .await;

            let res = self.handle_incoming_streams(incoming).await;
            self.disconnect(conn, None).await;
            res
        }
    }

    async fn handle_incoming_streams(
        &self,
        quic::IncomingStreams { mut bidi, mut uni }: quic::IncomingStreams<'_>,
    ) -> Result<(), Error> {
        future::try_join(
            async {
                while let Some(stream) = bidi.try_next().await? {
                    tracing::info!("new incoming bidi stream");
                    let this = self.clone();
                    tokio::spawn(async move {
                        if let Err(e) = this.incoming_bidi(stream).await {
                            tracing::warn!("incoming bidi stream error: {}", e);
                        }
                    });
                }

                Ok(())
            },
            async {
                while let Some(stream) = uni.try_next().await? {
                    tracing::info!("new incoming uni stream");
                    let this = self.clone();
                    tokio::spawn(async move {
                        if let Err(e) = this.incoming_uni(stream).await {
                            tracing::warn!("incoming uni stream error: {}", e);
                        }
                    });
                }

                Ok(())
            },
        )
        .await
        .map(|((), ())| ())
    }

    async fn outgoing_bidi(
        &self,
        stream: quic::BidiStream,
        hello: impl Into<Option<gossip::Rpc<IpAddr, A>>>,
    ) -> Result<(), Error> {
        match upgrade(stream, upgrade::Gossip).await {
            Err(upgrade::Error { stream, source }) => {
                stream.close(CloseReason::InvalidUpgrade);
                Err(Error::from(source))
            },

            Ok(upgraded) => self
                .gossip
                .outgoing_bidi(upgraded, hello)
                .await
                .map_err(|e| e.into()),
        }
    }

    async fn incoming_bidi(&self, stream: quic::BidiStream) -> Result<(), Error> {
        match with_upgraded(stream).await {
            Err(upgrade::Error { stream, source }) => {
                stream.close(CloseReason::InvalidUpgrade);
                Err(Error::from(source))
            },

            Ok(upgraded) => match upgraded {
                SomeUpgraded::Gossip(upgraded) => self
                    .gossip
                    .incoming_bidi(upgraded)
                    .await
                    .map_err(Error::Gossip),

                SomeUpgraded::Git(upgraded) => self
                    .git
                    .invoke_service(upgraded.into_stream().split())
                    .await
                    .map_err(Error::Git),
            },
        }
    }

    async fn incoming_uni(&self, stream: quic::RecvStream) -> Result<(), Error> {
        match with_upgraded(stream).await {
            Err(upgrade::Error { stream, source }) => {
                stream.close(CloseReason::InvalidUpgrade);
                Err(Error::from(source))
            },

            Ok(upgraded) => match upgraded {
                SomeUpgraded::Gossip(upgraded) => Ok(self.gossip.incoming_uni(upgraded).await?),

                SomeUpgraded::Git(upgraded) => {
                    upgraded.into_stream().close(CloseReason::InvalidUpgrade);
                    Err(Error::UnsupportedUpgrade)
                },
            },
        }
    }

    async fn open_bidi<U>(&self, to: PeerId, up: U) -> Result<Upgraded<U, quic::BidiStream>, Error>
    where
        U: Into<UpgradeRequest>,
    {
        let conn = self
            .connections
            .read()
            .unwrap()
            .get(&to)
            .map(Clone::clone)
            .ok_or(Error::NoConnection(to))?;
        let stream = conn.open_bidi().await?;
        upgrade_stream(stream, up).await
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

async fn connect_peer_info<'a>(
    endpoint: &quic::Endpoint,
    peer_info: gossip::PeerInfo<IpAddr>,
) -> Option<(quic::Connection, quic::IncomingStreams<'a>)> {
    let advertised_port = peer_info.advertised_info.listen_port;
    let addrs = iter::once(peer_info.advertised_info.listen_addr)
        .chain(peer_info.seen_addrs)
        .map(move |ip| SocketAddr::new(ip, advertised_port));
    connect(endpoint, peer_info.peer_id, addrs).await
}

async fn connect<'a, Addrs>(
    endpoint: &quic::Endpoint,
    peer_id: PeerId,
    addrs: Addrs,
) -> Option<(quic::Connection, quic::IncomingStreams<'a>)>
where
    Addrs: IntoIterator<Item = SocketAddr>,
{
    fn routable(addr: &SocketAddr) -> bool {
        let ip = addr.ip();
        !(ip.is_unspecified() || ip.is_documentation() || ip.is_multicast())
    }

    let addrs = addrs.into_iter().filter(routable).collect::<Vec<_>>();
    if addrs.is_empty() {
        tracing::warn!("no routable addrs for {}", peer_id);
        None
    } else {
        future::select_ok(addrs.iter().map(|addr| {
            let mut endpoint = endpoint.clone();
            tracing::info!(remote.id = %peer_id, remote.addr = %addr, "establishing connection");
            Box::pin(async move {
                endpoint
                    .connect(peer_id, &addr)
                    .map_err(|e| {
                        tracing::warn!("could not connect to {} at {}: {}", peer_id, addr, e);
                        e
                    })
                    .await
            })
        }))
        .await
        .ok()
        .map(|(success, _pending)| success)
    }
}

async fn upgrade_stream<S, U>(stream: S, up: U) -> Result<Upgraded<U, S>, Error>
where
    S: Closable + AsyncWrite + Unpin + Send + Sync,
    U: Into<UpgradeRequest>,
{
    upgrade(stream, up)
        .await
        .map_err(|upgrade::Error { stream, source }| {
            stream.close(CloseReason::InvalidUpgrade);
            Error::from(source)
        })
}
