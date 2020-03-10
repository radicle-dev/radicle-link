use std::{collections::HashMap, future::Future, net::SocketAddr, sync::Arc};

use async_trait::async_trait;
use failure::{format_err, Error};
use futures::{
    future::TryFutureExt,
    lock::Mutex,
    sink::SinkExt,
    stream::{BoxStream, StreamExt, TryStreamExt},
};
use futures_codec::CborCodec;
use log::{debug, error, info, trace, warn};
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

use crate::{
    git::{server::GitServer, transport::GitStreamFactory},
    internal::channel::Fanout,
    net::{
        connection::{BoundEndpoint, CloseReason, Connection, Endpoint, Stream},
        discovery::Discovery,
    },
    peer::PeerId,
};

pub mod rad;

#[derive(Debug, Clone, PartialEq, Eq, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum Upgrade {
    Rad = 0,
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

#[allow(clippy::large_enum_variant)]
enum Run<'a> {
    Discovered {
        peer: PeerId,
        addrs: Vec<SocketAddr>,
    },

    Incoming {
        conn: Connection,
        incoming: BoxStream<'a, Stream>,
    },

    Rad {
        event: rad::ProtocolEvent,
    },

    Shutdown,
}

#[derive(Clone)]
pub struct Protocol<S> {
    rad: rad::Protocol<S>,
    git: GitServer,

    connections: Arc<Mutex<HashMap<PeerId, Connection>>>,
    subscribers: Fanout<ProtocolEvent>,
}

impl<S> Protocol<S>
where
    S: rad::LocalStorage + 'static,
{
    pub fn new(rad: rad::Protocol<S>, git: GitServer) -> Self {
        Self {
            rad,
            git,
            connections: Arc::new(Mutex::new(HashMap::default())),
            subscribers: Fanout::new(),
        }
    }

    pub async fn run<Disco, Shutdown>(
        &mut self,
        BoundEndpoint { endpoint, incoming }: BoundEndpoint<'_>,
        disco: Disco,
        shutdown: Shutdown,
    ) where
        Disco: Discovery,
        Shutdown: Future<Output = ()> + Send,
    {
        debug!("Listening on {:?}", endpoint.local_addr());

        let incoming = incoming
            .filter_map(|(conn, i)| async move { Some(Run::Incoming { conn, incoming: i }) });
        let shutdown = futures::stream::once(shutdown).map(|()| Run::Shutdown);
        let bootstrap = futures::stream::iter(disco.collect())
            .map(|(peer, addrs)| Run::Discovered { peer, addrs });
        let rad_events = self.rad.subscribe().await.map(|event| Run::Rad { event });

        futures::stream::select(
            shutdown.map(Ok).boxed(),
            futures::stream::select(
                incoming.map(Ok).boxed(),
                futures::stream::select(bootstrap.map(Ok).boxed(), rad_events.map(Ok).boxed()),
            ),
        )
        .try_for_each_concurrent(None, |run| {
            let mut this = self.clone();
            let endpoint = endpoint.clone();
            async move { this.eval_run(endpoint, run).await }
        })
        .await
        .unwrap_or_else(|()| warn!("Shutting down"))
    }

    pub async fn subscribe(&self) -> impl futures::Stream<Item = ProtocolEvent> {
        self.subscribers.subscribe().await
    }

    pub async fn announce(&self, have: rad::Update) {
        self.rad.announce(have).await
    }

    pub async fn query(&self, want: rad::Update) {
        self.rad.query(want).await
    }

    pub async fn open_git(&self, to: &PeerId) -> Option<Stream> {
        trace!("Opening git stream to {}", to);
        async {
            if let Some(conn) = self.connections.lock().await.get(to) {
                trace!("Got connection to {}, getting stream", to);
                let s = conn
                    .open_stream()
                    .and_then(|stream| upgrade(stream, Upgrade::Git))
                    .await
                    .map_err(|e| error!("{}", e));

                trace!("Got git stream to {}: {}", to, s.is_ok());
                s.ok()
            } else {
                warn!("Not connected to {}", to);
                None
            }
        }
        .await
    }

    async fn eval_run(&mut self, endpoint: Endpoint, run: Run<'_>) -> Result<(), ()> {
        match run {
            Run::Discovered { peer, addrs } => {
                trace!("Run::Discovered: {}@{:?}", peer, addrs);
                if !self.connections.lock().await.contains_key(&peer) {
                    if let Some((conn, incoming)) = try_connect(&endpoint, &peer, &addrs).await {
                        self.handle_connect(conn, incoming.boxed(), None).await;
                    }
                }

                Ok(())
            },

            Run::Incoming { conn, incoming } => {
                trace!("Run::Incoming: {}", conn.remote_address());
                self.handle_incoming(conn, incoming).await;
                Ok(())
            },

            Run::Rad { event } => match event {
                rad::ProtocolEvent::SendAdhoc(info, rpc) => {
                    trace!("Run::Rad(SendAdhoc): {}", info.peer_id);
                    let conn = match self.connections.lock().await.get(&info.peer_id) {
                        Some(conn) => Some(conn.clone()),
                        None => {
                            match try_connect(
                                &endpoint,
                                &info.peer_id,
                                &info.seen_addrs.iter().cloned().collect::<Vec<SocketAddr>>(),
                            )
                            .await
                            {
                                Some((conn, _)) => Some(conn),
                                None => None,
                            }
                        },
                    };

                    if let Some(conn) = conn {
                        let stream = conn.open_stream().await.map_err(|e| {
                            warn!(
                                "Could not open stream on connection to {}: {}",
                                conn.remote_address(),
                                e
                            )
                        })?;

                        return self
                            .outgoing(stream, rpc)
                            .await
                            .map_err(|e| warn!("Error processing outgoing stream: {}", e));
                    }

                    Ok(())
                },

                rad::ProtocolEvent::Connect(info, rpc) => {
                    trace!("Run::Rad(Connect): {}", info.peer_id);
                    if !self.connections.lock().await.contains_key(&info.peer_id) {
                        let conn = try_connect(
                            &endpoint,
                            &info.peer_id,
                            &info.seen_addrs.iter().cloned().collect::<Vec<SocketAddr>>(),
                        )
                        .await;

                        if let Some((conn, incoming)) = conn {
                            self.handle_connect(conn, incoming.boxed(), Some(rpc)).await
                        }
                    }

                    Ok(())
                },

                rad::ProtocolEvent::Disconnect(peer) => {
                    trace!("Run::Rad(Disconnect): {}", peer);
                    self.handle_disconnect(peer).await;
                    Ok(())
                },
            },

            Run::Shutdown => {
                debug!("Run::Shutdown");
                Err(())
            },
        }
    }

    async fn handle_connect(
        &self,
        conn: Connection,
        mut incoming: BoxStream<'_, Stream>,
        hello: impl Into<Option<rad::Rpc>>,
    ) {
        let remote_id = conn.peer_id().clone();
        let remote_addr = conn.remote_address();

        info!("New outgoing connection: {}@{}", remote_id, remote_addr,);

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
                while let Some(stream) = incoming.next().await {
                    this2.incoming(stream).await?
                }

                Ok(())
            }
        );

        if let Err(e) = res {
            warn!("Closing connection with {}, because: {}", remote_id, e);
            conn.close(CloseReason::ConnectionError);
        };
    }

    async fn handle_disconnect(&self, peer: PeerId) {
        if let Some(conn) = self.connections.lock().await.remove(&peer) {
            info!("Disconnecting: {}", conn.remote_address());
            // FIXME: make this more graceful
            conn.close(CloseReason::ProtocolDisconnect);
            self.subscribers
                .emit(ProtocolEvent::Disconnected(peer))
                .await
        }
    }

    async fn handle_incoming<Incoming>(&self, conn: Connection, mut incoming: Incoming)
    where
        Incoming: futures::Stream<Item = Stream> + Unpin,
    {
        let remote_id = conn.peer_id().clone();
        let remote_addr = conn.remote_address();

        info!("New incoming connection: {}@{}", remote_id, remote_addr);

        {
            self.connections
                .lock()
                .await
                .insert(remote_id.clone(), conn);
            self.subscribers
                .emit(ProtocolEvent::Connected(remote_id.clone()))
                .await;
        }

        while let Some(stream) = incoming.next().await {
            debug!("New incoming stream");
            let this = self.clone();
            tokio::spawn(async move {
                if let Err(e) = this.incoming(stream).await {
                    warn!("Incoming stream error: {}", e);
                }
            });
        }

        trace!("Incoming from {} done", remote_addr)
    }

    async fn outgoing(
        &mut self,
        stream: Stream,
        hello: impl Into<Option<rad::Rpc>>,
    ) -> Result<(), Error> {
        let upgraded = upgrade(stream, Upgrade::Rad).await?;
        self.rad
            .outgoing(upgraded.framed(CborCodec::new()), hello)
            .await
            .map_err(|e| e.into())
    }

    async fn incoming(&self, stream: Stream) -> Result<(), Error> {
        let mut stream = stream.framed(CborCodec::<UpgradeResponse, Upgrade>::new());
        match stream.try_next().await {
            Ok(resp) => match resp {
                Some(upgrade) => {
                    stream
                        .send(UpgradeResponse::SwitchingProtocols(upgrade.clone()))
                        .await
                        .map_err(|e| format_err!("Failed to send upgrade response: {:?}", e))?;

                    // remove framing
                    let stream = stream.release().0;
                    match upgrade {
                        Upgrade::Rad => {
                            trace!("Incoming stream updgraded to rad");
                            self.rad
                                .incoming(stream.framed(CborCodec::new()))
                                .await
                                .map_err(|e| format_err!("Error handling rad upgrade: {}", e))
                        },

                        Upgrade::Git => {
                            trace!("Incoming stream upgraded to git");
                            self.git
                                .invoke_service(stream.split())
                                .await
                                .map_err(|e| format_err!("Error handling git upgrade: {}", e))
                        },
                    }
                },

                None => Err(format_err!("Silent client")),
            },

            Err(e) => {
                let _ = stream
                    .send(UpgradeResponse::Error(UpgradeError::InvalidPayload))
                    .await;
                Err(format_err!("Error deserialising upgrade request: {:?}", e))
            },
        }
    }
}

#[async_trait]
impl<S> GitStreamFactory for Protocol<S>
where
    S: rad::LocalStorage + 'static,
{
    async fn open_stream(&self, to: &PeerId) -> Option<Stream> {
        self.open_git(to).await
    }
}

async fn try_connect(
    endpoint: &Endpoint,
    peer_id: &PeerId,
    addrs: &[SocketAddr],
) -> Option<(Connection, impl futures::Stream<Item = Stream> + Unpin)> {
    futures::stream::iter(addrs)
        .filter_map(|addr| {
            let mut endpoint = endpoint.clone();
            info!("Connecting to: {}@{}", peer_id, addr);
            Box::pin(async move {
                match endpoint.connect(peer_id, &addr).await {
                    Ok(conn) => Some(conn),
                    Err(e) => {
                        warn!("Could not connect to {} at {}: {}", peer_id, addr, e);
                        None
                    },
                }
            })
        })
        .next()
        .await
}

async fn upgrade(stream: Stream, upgrade: Upgrade) -> Result<Stream, Error> {
    trace!("Upgrade to {:?}", upgrade);

    let mut stream = stream.framed(CborCodec::<Upgrade, UpgradeResponse>::new());

    stream
        .send(upgrade.clone())
        .await
        .map_err(|e| format_err!("Failed to send upgrade {:?}: {:?}", upgrade, e))?;

    match stream.try_next().await {
        Ok(resp) => match resp {
            Some(UpgradeResponse::SwitchingProtocols(proto)) => {
                if proto == upgrade {
                    Ok(stream.release().0)
                } else {
                    Err(format_err!(
                        "Protocol mismatch: requested {:?}, got {:?}",
                        Upgrade::Rad,
                        upgrade
                    ))
                }
            },
            Some(UpgradeResponse::Error(e)) => Err(format_err!("Peer denied rad upgrade: {:?}", e)),
            None => Err(format_err!("Silent server")),
        },

        Err(e) => Err(format_err!("Error deserialising upgrade response: {:?}", e)),
    }
}
