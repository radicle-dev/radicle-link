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
    io,
    iter,
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use futures::{
    future::TryFutureExt,
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
        upgrade::{self, upgrade, with_upgrade, UpgradeRequest, Upgraded, WithUpgrade},
    },
    peer::PeerId,
};

#[derive(Debug, Clone)]
pub enum ProtocolEvent<A> {
    Connected(PeerId),
    Disconnected(PeerId),
    Listening(SocketAddr),
    Gossip(gossip::Info<IpAddr, A>),
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
    #[error("No connection to {0}")]
    NoConnection(PeerId),

    #[error(transparent)]
    Upgrade(#[from] upgrade::Error),

    #[error(transparent)]
    Cbor(#[from] CborCodecError),

    #[error("Error handling gossip upgrade")]
    Gossip(#[from] gossip::error::Error),

    #[error("Error handling git upgrade")]
    Git(#[source] io::Error),

    #[error(transparent)]
    Quic(#[from] quic::Error),

    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(Clone)]
pub struct Protocol<S, A> {
    gossip: gossip::Protocol<S, A, IpAddr, quic::RecvStream, quic::SendStream>,
    git: GitServer,

    connections: Arc<Mutex<HashMap<PeerId, quic::Connection>>>,
    subscribers: Fanout<ProtocolEvent<A>>,
}

impl<S, A> Protocol<S, A>
where
    S: gossip::LocalStorage<Update = A> + 'static,
    for<'de> A: Encode + Decode<'de> + Clone + Debug + Send + Sync + 'static,
{
    pub fn new(
        gossip: gossip::Protocol<S, A, IpAddr, quic::RecvStream, quic::SendStream>,
        git: GitServer,
    ) -> Self {
        Self {
            gossip,
            git,
            connections: Arc::new(Mutex::new(HashMap::default())),
            subscribers: Fanout::new(),
        }
    }

    pub fn peer_id(&self) -> &PeerId {
        self.gossip.peer_id()
    }

    /// Start the protocol run loop.
    pub async fn run<Disco>(
        self,
        quic::BoundEndpoint { endpoint, incoming }: quic::BoundEndpoint<'_>,
        disco: Disco,
    ) where
        Disco: futures::stream::Stream<Item = (PeerId, Vec<SocketAddr>)>,
    {
        let local_addr = endpoint
            .local_addr()
            .expect("unable to get local endpoint addr");

        let span = tracing::info_span!(
            "Protocol::run",
            local.id = %self.peer_id(),
            local.addr = %local_addr
        );

        async move {
            let incoming = incoming.map(|(conn, i)| Run::Incoming { conn, incoming: i });
            let bootstrap = disco.map(|(peer, addrs)| Run::Discovered { peer, addrs });
            let gossip_events = self
                .gossip
                .subscribe()
                .await
                .map(|event| Run::Gossip { event });

            tracing::info!("Listening");
            self.subscribers
                .emit(ProtocolEvent::Listening(local_addr))
                .await;

            futures::stream::select(incoming, futures::stream::select(bootstrap, gossip_events))
                .for_each_concurrent(None, |run| {
                    let mut this = self.clone();
                    let endpoint = endpoint.clone();
                    async move { this.eval_run(endpoint, run).await }
                })
                .await
        }
        .instrument(span)
        .await
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
    pub async fn open_git(
        &self,
        to: &PeerId,
    ) -> Result<Upgraded<quic::Stream, upgrade::Git>, Error> {
        self.open_stream(to, upgrade::Git).await
    }

    async fn eval_run(&mut self, endpoint: quic::Endpoint, run: Run<'_, A>) {
        match run {
            Run::Discovered { peer, addrs } => {
                tracing::trace!(
                    remote.id = %peer,
                    remote.addrs = ?addrs,
                    "Run::Discovered",
                );

                if !self.connections.lock().await.contains_key(&peer) {
                    if let Some((conn, incoming)) = connect(&endpoint, &peer, addrs).await {
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
                            None => connect_peer_info(&endpoint, to).await.map(|(conn, _)| conn),
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

                    gossip::Control::Connect { to, hello } => {
                        tracing::trace!(remote.id = %to.peer_id, "Run::Rad(Connect)");

                        if !self.connections.lock().await.contains_key(&to.peer_id) {
                            let conn = connect_peer_info(&endpoint, to).await;
                            if let Some((conn, incoming)) = conn {
                                self.handle_connect(conn, incoming.boxed(), Some(hello))
                                    .await
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
            },
        }
    }

    async fn handle_connect(
        &self,
        conn: quic::Connection,
        mut incoming: BoxStream<'_, quic::Result<quic::Stream>>,
        hello: impl Into<Option<gossip::Rpc<IpAddr, A>>>,
    ) {
        let remote_id = conn.remote_peer_id().clone();
        tracing::info!(remote.id = %remote_id, "New outgoing connection");

        {
            self.connections
                .lock()
                .await
                .insert(remote_id.clone(), conn.clone());
            self.subscribers
                .emit(ProtocolEvent::Connected(remote_id.clone()))
                .await;
        }

        let mut this1 = self.clone();
        let this2 = self.clone();

        let res = futures::try_join!(
            async {
                let outgoing = conn.open_stream().await?;
                this1.outgoing(outgoing, hello).await
            },
            async {
                while let Some(stream) = incoming.try_next().await? {
                    this2.incoming(stream).await?
                }

                Ok(())
            }
        );

        if let Err(e) = res {
            tracing::warn!("Closing connection with {}, because: {}", remote_id, e);
            conn.close(CloseReason::InternalError);
        };
    }

    async fn handle_disconnect(&self, peer: PeerId) {
        if let Some(conn) = self.connections.lock().await.remove(&peer) {
            tracing::info!(msg = "Disconnecting", remote.addr = %conn.remote_addr());
            // FIXME: make this more graceful
            conn.close(CloseReason::ProtocolDisconnect);
            self.subscribers
                .emit(ProtocolEvent::Disconnected(peer))
                .await
        }
    }

    async fn handle_incoming<Incoming>(
        &self,
        conn: quic::Connection,
        mut incoming: Incoming,
    ) -> Result<(), Error>
    where
        Incoming: futures::Stream<Item = quic::Result<quic::Stream>> + Unpin,
    {
        let remote_id = conn.remote_peer_id().clone();
        tracing::info!(remote.id = %remote_id, "New incoming connection");

        {
            self.connections
                .lock()
                .await
                .insert(remote_id.clone(), conn);
            self.subscribers
                .emit(ProtocolEvent::Connected(remote_id.clone()))
                .await;
        }

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
        &mut self,
        stream: quic::Stream,
        hello: impl Into<Option<gossip::Rpc<IpAddr, A>>>,
    ) -> Result<(), Error> {
        let upgraded = upgrade(stream, upgrade::Gossip).await?;
        self.gossip
            .outgoing(upgraded, hello)
            .await
            .map_err(|e| e.into())
    }

    async fn incoming(&self, stream: quic::Stream) -> Result<(), Error> {
        match with_upgrade(stream).await? {
            WithUpgrade::Gossip(upgraded) => self
                .gossip
                .incoming(upgraded.await?)
                .await
                .map_err(Error::Gossip),

            WithUpgrade::Git(upgraded) => self
                .git
                .invoke_service(upgraded.await?.into_stream().split())
                .await
                .map_err(Error::Git),
        }
    }

    async fn open_stream<U>(&self, to: &PeerId, up: U) -> Result<Upgraded<quic::Stream, U>, Error>
    where
        U: Into<UpgradeRequest>,
    {
        match self.connections.lock().await.get(to) {
            Some(conn) => {
                conn.open_stream()
                    .map_err(|e| e.into())
                    .and_then(|stream| upgrade(stream, up))
                    .map_err(|e| e.into())
                    .await
            },
            None => Err(Error::NoConnection(to.clone())),
        }
    }
}

#[async_trait]
impl<S, A> GitStreamFactory for Protocol<S, A>
where
    S: gossip::LocalStorage<Update = A> + 'static,
    for<'de> A: Encode + Decode<'de> + Clone + Debug + Send + Sync + 'static,
{
    async fn open_stream(&self, to: &PeerId) -> Option<Box<dyn GitStream>> {
        let span = tracing::trace_span!("GitStreamFactory::open_stream", peer.id = %to);
        let _guard = span.enter();

        match self.open_git(to).await {
            Ok(s) => Some(Box::new(s)),
            Err(e) => {
                tracing::warn!("Error opening git stream: {}", e);
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
    connect(endpoint, &peer_info.peer_id, addrs).await
}

async fn connect<I>(
    endpoint: &quic::Endpoint,
    peer_id: &PeerId,
    addrs: I,
) -> Option<(
    quic::Connection,
    impl futures::Stream<Item = quic::Result<quic::Stream>> + Unpin,
)>
where
    I: IntoIterator<Item = SocketAddr>,
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
