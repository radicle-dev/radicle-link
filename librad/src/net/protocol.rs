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

use std::{collections::HashMap, fmt::Debug, future::Future, io, iter, net::SocketAddr, sync::Arc};

use async_trait::async_trait;
use futures::{
    future::TryFutureExt,
    lock::Mutex,
    sink::SinkExt,
    stream::{BoxStream, StreamExt, TryStreamExt},
};
use futures_codec::{CborCodec, CborCodecError, FramedRead, FramedWrite};
// use log::{debug, error, info, trace, warn};
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use thiserror::Error;
use tracing;

use crate::{
    channel::Fanout,
    git::{
        server::GitServer,
        transport::{GitStream, GitStreamFactory},
    },
    net::{
        connection::{CloseReason, LocalInfo, RemoteInfo, Stream},
        gossip,
        quic,
    },
    peer::PeerId,
};

/// We support on-way protocol upgrades on individual QUIC streams (irrespective
/// of ALPN, which applies per-connection).
#[derive(Debug, Clone, PartialEq, Eq, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum Upgrade {
    Gossip = 0,
    Git = 1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpgradeResponse {
    // TODO(kim): Technically, we don't need a confirmation. Keeping it here for
    // now, so we can send back an error. Maybe we'll also need some additional
    // response payload in the future, who knows.
    SwitchingProtocols(Upgrade),
    Error(UpgradeError),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpgradeError {
    InvalidPayload,
    UnsupportedUpgrade(Upgrade), // reserved
}

#[derive(Debug, Clone)]
pub enum ProtocolEvent {
    Connected(PeerId),
    Disconnected(PeerId),
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
        event: gossip::ProtocolEvent<A>,
    },

    Shutdown,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Upgrade: protocol mismatch, expected {expected:?}, got {actual:?}")]
    UpgradeProtocolMismatch { expected: Upgrade, actual: Upgrade },

    #[error("Peer denied upgrade: {0:?}")]
    UpgradeErrorResponse(UpgradeError),

    #[error("Silent server")]
    SilentServer,
    #[error("Silent client")]
    SilentClient,

    #[error(transparent)]
    Cbor(#[from] serde_cbor::Error),

    #[error("Error handling gossip upgrade: {0}")]
    Gossip(#[from] gossip::error::Error),

    #[error("Error handling git upgrade: {0}")]
    Git(#[source] io::Error),

    #[error(transparent)]
    Quic(#[from] quic::Error),

    #[error(transparent)]
    Io(#[from] io::Error),
}

impl From<CborCodecError> for Error {
    fn from(e: CborCodecError) -> Self {
        match e {
            CborCodecError::Cbor(e) => Self::Cbor(e),
            CborCodecError::Io(e) => Self::Io(e),
        }
    }
}

#[derive(Clone)]
pub struct Protocol<S, A> {
    gossip: gossip::Protocol<S, A, quic::RecvStream, quic::SendStream>,
    git: GitServer,

    connections: Arc<Mutex<HashMap<PeerId, quic::Connection>>>,
    subscribers: Fanout<ProtocolEvent>,
}

impl<S, A> Protocol<S, A>
where
    S: gossip::LocalStorage<Update = A> + 'static,
    for<'de> A: Serialize + Deserialize<'de> + Clone + Debug + Send + Sync + 'static,
{
    pub fn new(
        gossip: gossip::Protocol<S, A, quic::RecvStream, quic::SendStream>,
        git: GitServer,
    ) -> Self {
        Self {
            gossip,
            git,
            connections: Arc::new(Mutex::new(HashMap::default())),
            subscribers: Fanout::new(),
        }
    }

    /// Start the protocol run loop.
    ///
    /// This method terminates when the `Shutdown` future resolves, the program
    /// is interrupted, or an error occurs.
    pub async fn run<Disco, Shutdown>(
        &mut self,
        quic::BoundEndpoint { endpoint, incoming }: quic::BoundEndpoint<'_>,
        disco: Disco,
        shutdown: Shutdown,
    ) where
        Disco: futures::stream::Stream<Item = (PeerId, Vec<SocketAddr>)>,
        Shutdown: Future<Output = ()> + Send,
    {
        let span = tracing::trace_span!("Protocol::run");
        let _guard = span.enter();

        let incoming = incoming.map(|(conn, i)| Ok(Run::Incoming { conn, incoming: i }));
        let shutdown = futures::stream::once(shutdown).map(|()| Ok(Run::Shutdown));
        let bootstrap = disco.map(|(peer, addrs)| Ok(Run::Discovered { peer, addrs }));
        let gossip_events = self
            .gossip
            .subscribe()
            .await
            .map(|event| Ok(Run::Gossip { event }));

        tracing::info!(msg = "Listening", local.addr = ?endpoint.local_addr());

        futures::stream::select(
            shutdown,
            futures::stream::select(incoming, futures::stream::select(bootstrap, gossip_events)),
        )
        .try_for_each_concurrent(None, |run| {
            let mut this = self.clone();
            let endpoint = endpoint.clone();
            async move { this.eval_run(endpoint, run).await }
        })
        .await
        .unwrap_or_else(|()| tracing::warn!("Shutting down"))
    }

    /// Subscribe to an infinite stream of [`ProtocolEvent`]s.
    ///
    /// The consumer must keep polling the stream, or drop it to cancel the
    /// subscription.
    pub async fn subscribe(&self) -> impl futures::Stream<Item = ProtocolEvent> {
        self.subscribers.subscribe().await
    }

    /// Announce an update to the network
    pub async fn announce(&self, have: A) {
        self.gossip.announce(have).await
    }

    /// Query the network for an update
    ///
    /// The returned stream will yield an infinite sequence of reponses from
    /// peers claiming to provide the requested value. The stream can safely
    /// be dropped at any point.
    ///
    /// Note that responses will also cause [`gossip::LocalStorage::put`] to be
    /// invoked, i.e. the local storage will be converged towards the
    /// requested state.
    pub async fn query(&self, want: A) -> impl futures::Stream<Item = gossip::Has<A>> {
        self.gossip.query(want).await;
        self.gossip.subscribe().await.filter_map(|evt| async move {
            match evt {
                gossip::ProtocolEvent::Info(gossip::Info::Has(has)) => Some(has),
                _ => None,
            }
        })
    }

    /// Open a QUIC stream which is upgraded to expect the git protocol
    pub async fn open_git(&self, to: &PeerId) -> Option<quic::Stream> {
        tracing::trace!("Opening git stream");
        if let Some(conn) = self.connections.lock().await.get(to) {
            conn.open_stream()
                .map_err(|e| e.into())
                .and_then(|stream| upgrade(stream, Upgrade::Git))
                .await
                .map_err(|e| tracing::error!("{}", e))
                .ok()
        } else {
            tracing::warn!("Error opening git stream: not connected to {}", to);
            None
        }
    }

    async fn eval_run(&mut self, endpoint: quic::Endpoint, run: Run<'_, A>) -> Result<(), ()> {
        match run {
            Run::Discovered { peer, addrs } => {
                let span = tracing::trace_span!(
                    "Run::Discovered",
                    peer.id = %peer,
                    peer.addrs = ?addrs
                );
                let _guard = span.enter();

                if !self.connections.lock().await.contains_key(&peer) {
                    if let Some((conn, incoming)) = connect(&endpoint, &peer, addrs).await {
                        self.handle_connect(conn, incoming.boxed(), None).await;
                    }
                }

                Ok(())
            },

            Run::Incoming { conn, incoming } => {
                let span = tracing::trace_span!("Run::Incoming", peer.id = %conn.remote_peer_id(), peer.addrs = %conn.remote_addr());
                let _guard = span.enter();

                self.handle_incoming(conn, incoming)
                    .await
                    .map_err(|e| tracing::warn!("Error processing incoming connection: {}", e))
            },

            Run::Gossip { event } => match event {
                gossip::ProtocolEvent::Control(ctrl) => match ctrl {
                    gossip::Control::SendAdhoc { to, rpc } => {
                        tracing::trace!("Run::Rad(SendAdhoc)");
                        let conn = match self.connections.lock().await.get(&to.peer_id) {
                            Some(conn) => Some(conn.clone()),
                            None => connect_peer_info(&endpoint, to).await.map(|(conn, _)| conn),
                        };

                        if let Some(conn) = conn {
                            let stream = conn.open_stream().await.map_err(|e| {
                                tracing::warn!(
                                    "Could not open stream on connection to {}: {}",
                                    conn.remote_addr(),
                                    e
                                )
                            })?;

                            return self.outgoing(stream, rpc).await.map_err(|e| {
                                tracing::warn!("Error processing outgoing stream: {}", e)
                            });
                        }

                        Ok(())
                    },

                    gossip::Control::Connect { to, hello } => {
                        tracing::trace!("Run::Rad(Connect)");
                        if !self.connections.lock().await.contains_key(&to.peer_id) {
                            let conn = connect_peer_info(&endpoint, to).await;
                            if let Some((conn, incoming)) = conn {
                                self.handle_connect(conn, incoming.boxed(), Some(hello))
                                    .await
                            }
                        }

                        Ok(())
                    },

                    gossip::Control::Disconnect(peer) => {
                        tracing::trace!("Run::Rad(Disconnect)");
                        self.handle_disconnect(peer).await;
                        Ok(())
                    },
                },

                gossip::ProtocolEvent::Info(_) => Ok(()),
            },

            Run::Shutdown => {
                tracing::debug!("Run::Shutdown");
                Err(())
            },
        }
    }

    async fn handle_connect(
        &self,
        conn: quic::Connection,
        mut incoming: BoxStream<'_, quic::Result<quic::Stream>>,
        hello: impl Into<Option<gossip::Rpc<A>>>,
    ) {
        tracing::info!("New outgoing connection");
        let remote_id = conn.remote_peer_id().clone();

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

        tracing::info!("New incoming connection");

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
        hello: impl Into<Option<gossip::Rpc<A>>>,
    ) -> Result<(), Error> {
        let upgraded = upgrade(stream, Upgrade::Gossip).await?;
        let (recv, send) = upgraded.split();
        self.gossip
            .outgoing(
                FramedRead::new(recv, CborCodec::new()),
                FramedWrite::new(send, CborCodec::new()),
                hello,
            )
            .await
            .map_err(|e| e.into())
    }

    async fn incoming(&self, stream: quic::Stream) -> Result<(), Error> {
        let mut stream = stream.framed(CborCodec::<UpgradeResponse, Upgrade>::new());
        match stream.try_next().await {
            Ok(resp) => match resp {
                Some(upgrade) => {
                    stream
                        .send(UpgradeResponse::SwitchingProtocols(upgrade.clone()))
                        .await?;

                    tracing::trace!(msg = "Incoming stream upgraded", upgrade = ?upgrade);

                    // remove framing
                    let stream = stream.release().0;
                    match upgrade {
                        Upgrade::Gossip => {
                            let (recv, send) = stream.split();
                            self.gossip
                                .incoming(
                                    FramedRead::new(recv, CborCodec::new()),
                                    FramedWrite::new(send, CborCodec::new()),
                                )
                                .await
                                .map_err(Error::Gossip)
                        },

                        Upgrade::Git => self
                            .git
                            .invoke_service(stream.split())
                            .await
                            .map_err(Error::Git),
                    }
                },

                None => Err(Error::SilentClient),
            },

            Err(e) => {
                let _ = stream
                    .send(UpgradeResponse::Error(UpgradeError::InvalidPayload))
                    .await;
                Err(e.into())
            },
        }
    }
}

#[async_trait]
impl<S, A> GitStreamFactory for Protocol<S, A>
where
    S: gossip::LocalStorage<Update = A> + 'static,
    for<'de> A: Serialize + Deserialize<'de> + Clone + Debug + Send + Sync + 'static,
{
    async fn open_stream(&self, to: &PeerId) -> Option<Box<dyn GitStream>> {
        let span = tracing::trace_span!("GitStreamFactory::open_stream", peer.id = %to);
        let _guard = span.enter();

        // Nb.: type inference fails if this is not a pattern match (ie. `map`)
        match self.open_git(to).await {
            Some(s) => Some(Box::new(s)),
            None => None,
        }
    }
}

async fn connect_peer_info(
    endpoint: &quic::Endpoint,
    peer_info: gossip::PeerInfo,
) -> Option<(
    quic::Connection,
    impl futures::Stream<Item = quic::Result<quic::Stream>> + Unpin,
)> {
    let addrs = iter::once(peer_info.advertised_info.listen_addr).chain(peer_info.seen_addrs);
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
            tracing::info!("Establishing connection");
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

async fn upgrade(stream: quic::Stream, upgrade: Upgrade) -> Result<quic::Stream, Error> {
    tracing::trace!(msg = "Upgrade", upgrade = ?upgrade);

    let mut stream = stream.framed(CborCodec::<Upgrade, UpgradeResponse>::new());
    stream.send(upgrade.clone()).await?;
    let resp = stream.try_next().await?;
    if let Some(resp) = resp {
        match resp {
            UpgradeResponse::SwitchingProtocols(proto) => {
                if proto == upgrade {
                    Ok(stream.release().0)
                } else {
                    Err(Error::UpgradeProtocolMismatch {
                        expected: Upgrade::Gossip,
                        actual: upgrade,
                    })
                }
            },
            UpgradeResponse::Error(e) => Err(Error::UpgradeErrorResponse(e)),
        }
    } else {
        Err(Error::SilentServer)
    }
}
