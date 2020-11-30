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
    net::SocketAddr,
    pin::Pin,
    sync::{
        atomic::{self, AtomicUsize},
        Arc,
    },
    task::{Context, Poll},
};

use futures::{
    future::{self, BoxFuture, FutureExt as _},
    stream::{StreamExt as _, TryStreamExt as _},
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
        connection::{CloseReason, Duplex as _, LocalInfo, RemoteInfo, RemotePeer as _},
        conntrack,
        gossip,
        quic,
        upgrade::{self, upgrade, with_upgraded, SomeUpgraded, UpgradeRequest, Upgraded},
    },
    peer::PeerId,
};

pub mod error;
pub use error::Error;

mod handler;
mod io;

#[derive(Clone, Debug)]
pub enum ProtocolEvent<A> {
    Connected(PeerId),
    Disconnecting(PeerId),
    Listening(SocketAddr),
    Gossip(gossip::Info<SocketAddr, A>),
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
        event: gossip::ProtocolEvent<SocketAddr, A>,
    },
}

pub type RunLoop = BoxFuture<'static, ()>;

pub struct Protocol<S, A> {
    gossip: gossip::Protocol<S, A, SocketAddr, quic::BidiStream>,
    git: GitServer,

    endpoint: quic::Endpoint,

    connections: conntrack::Connections,
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
                tracing::warn!("shutting down endpoint");
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
        gossip: gossip::Protocol<S, A, SocketAddr, quic::BidiStream>,
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
            connections: conntrack::Connections::default(),
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
            .lock_iter()
            .iter()
            .map(|(peer_id, conn)| (*peer_id, conn.remote_addr()))
            .collect()
    }

    /// Returns `true` if there is at least one active connection.
    pub async fn has_connections(&self) -> bool {
        !self.connections.is_empty()
    }

    /// Returns the number of currently active connections.
    pub fn num_connections(&self) -> usize {
        self.connections.len()
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
                    let (conn, incoming) = io::connect(&self.endpoint, to, addr_hints)
                        .await
                        .ok_or(Error::NoConnection(to))?;

                    let stream = conn.open_bidi().await?;
                    let upgraded = io::upgrade_stream(stream, upgrade::Git).await?;
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
                if let Some((conn, incoming)) = io::connect(&self.endpoint, peer, addrs).await {
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

                        let conn = match self.connections.get(&to_peer) {
                            Some(conn) => Some(conn),
                            None if to.peer_id != self.peer_id() => {
                                tracing::info!("adhoc requires new connection");
                                io::connect_peer_info(&self.endpoint, to)
                                    .await
                                    .map(|(conn, _)| conn)
                            },
                            None => {
                                tracing::warn!("BUG: gossip requested self-connection");
                                None
                            },
                        };

                        match conn {
                            None => tracing::warn!("failed to obtain connection for adhoc rpc"),
                            Some(conn) => async {
                                let stream = conn.open_uni().await.map_err(Error::from)?;
                                let upgraded = io::upgrade_stream(stream, upgrade::Gossip).await?;
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

                        match self.connections.get(&to.peer_id) {
                            None => {
                                tracing::info!("no matching connection to {}", to.peer_id);
                                let conn = io::connect_peer_info(&self.endpoint, to).await;
                                if let Some((conn, incoming)) = conn {
                                    self.handle_connect(conn, incoming, None).await
                                }
                            },

                            Some(conn) => {
                                tracing::info!("reusing existing connection to {}", to.peer_id);
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

                    gossip::Control::Disconnect { peer } => {
                        tracing::trace!(remote.id = %peer, "Run::Rad(Disconnect)");

                        if let Some(conn) = self.connections.get(&peer) {
                            tracing::info!("disconnecting {}", peer);
                            conn.close(CloseReason::ProtocolDisconnect);
                        }
                    },
                },

                gossip::ProtocolEvent::Info(info) => {
                    self.subscribers.emit(ProtocolEvent::Gossip(info)).await
                },
            },
        }
    }

    async fn handle_connect<'a>(
        &'a self,
        conn: quic::Connection,
        incoming: quic::IncomingStreams<'a>,
        hello: impl Into<Option<gossip::Rpc<SocketAddr, A>>>,
    ) {
        let remote_id = conn.remote_peer_id();
        tracing::info!(remote.id = %remote_id, "new outgoing connection");

        {
            let _prev = self.connections.insert(conn.clone());
            if let Some(prev) = _prev {
                tracing::warn!(
                    "new outgoing ejects previous connection to {} @ {}",
                    remote_id,
                    prev.remote_addr()
                );
                prev.close(CloseReason::DuplicateConnection);
            }
        }

        self.subscribers
            .emit(ProtocolEvent::Connected(remote_id))
            .await;

        let res = futures::try_join!(
            async {
                let hello = hello.into();
                loop {
                    let outgoing = conn.open_bidi().await?;
                    let stream_id = outgoing.id();
                    match self.outgoing_bidi(outgoing, hello.clone()).await {
                        Ok(()) => tracing::info!(
                            remote_id = %remote_id,
                            stream_id = ?stream_id,
                            "outgoing stream ended, restarting"
                        ),
                        Err(e) => return Err::<(), Error>(e),
                    }
                }
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
            let was_removed = self.connections.remove(&conn);
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

    #[tracing::instrument(
        skip(self, conn, incoming),
        fields(
            remote_peer = %conn.remote_peer_id(),
            remote_addr = %conn.remote_addr(),
        ),
        err
    )]
    async fn handle_incoming(
        &self,
        conn: quic::Connection,
        incoming: quic::IncomingStreams<'_>,
    ) -> Result<(), Error> {
        let remote_id = conn.remote_peer_id();
        tracing::info!("new incoming connection");

        if let Some(prev) = self.connections.insert(conn.clone()) {
            prev.close(CloseReason::DuplicateConnection);
        }

        self.subscribers
            .emit(ProtocolEvent::Connected(remote_id))
            .await;

        let res = self.handle_incoming_streams(incoming).await;
        //self.disconnect(conn, None).await;
        res
    }

    async fn handle_incoming_streams(
        &self,
        quic::IncomingStreams { mut bidi, mut uni }: quic::IncomingStreams<'_>,
    ) -> Result<(), Error> {
        future::try_join(
            async {
                while let Some(stream) = bidi.try_next().await? {
                    let remote_id = stream.remote_peer_id();
                    let remote_addr = stream.remote_addr();
                    tracing::info!(
                        remote_id = %remote_id,
                        remote_addr = %remote_addr,
                        "new incoming bidi stream"
                    );
                    let this = self.clone();
                    tokio::spawn(async move {
                        if let Err(err) = this.incoming_bidi(stream).await {
                            tracing::warn!(
                                remote_id = %remote_id,
                                remote_addr = %remote_addr,
                                err = ?err,
                                "incoming bidi stream error"
                            );
                        }
                    });
                }

                Ok(())
            },
            async {
                while let Some(stream) = uni.try_next().await? {
                    let remote_id = stream.remote_peer_id();
                    let remote_addr = stream.remote_addr();
                    tracing::info!(
                        remote_id = %remote_id,
                        remote_addr = %remote_addr,
                        "new incoming uni stream"
                    );
                    let this = self.clone();
                    tokio::spawn(async move {
                        if let Err(err) = this.incoming_uni(stream).await {
                            tracing::warn!(
                                remote_id = %remote_id,
                                remote_addr = %remote_addr,
                                err = ?err,
                                "incoming uni stream error"
                            );
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
        hello: impl Into<Option<gossip::Rpc<SocketAddr, A>>>,
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
        let conn = self.connections.get(&to).ok_or(Error::NoConnection(to))?;
        let stream = conn.open_bidi().await?;
        io::upgrade_stream(stream, up).await
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
