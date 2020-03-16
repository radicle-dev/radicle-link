use std::{collections::HashMap, future::Future, iter, net::SocketAddr, sync::Arc};

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
    channel::Fanout,
    git::{server::GitServer, transport::GitStreamFactory},
    net::{
        connection::{BoundEndpoint, CloseReason, Connection, Endpoint, Stream},
        discovery::Discovery,
        gossip,
    },
    peer::PeerId,
};

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

enum Run<'a> {
    Discovered {
        peer: PeerId,
        addrs: Vec<SocketAddr>,
    },

    Incoming {
        conn: Connection,
        incoming: BoxStream<'a, Result<Stream, Error>>,
    },

    Gossip {
        event: gossip::ProtocolEvent,
    },

    Shutdown,
}

#[derive(Clone)]
pub struct Protocol<S> {
    gossip: gossip::Protocol<S>,
    git: GitServer,

    connections: Arc<Mutex<HashMap<PeerId, Connection>>>,
    subscribers: Fanout<ProtocolEvent>,
}

impl<S> Protocol<S>
where
    S: gossip::LocalStorage + 'static,
{
    pub fn new(gossip: gossip::Protocol<S>, git: GitServer) -> Self {
        Self {
            gossip,
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
        let incoming = incoming.map(|(conn, i)| Ok(Run::Incoming { conn, incoming: i }));
        let shutdown = futures::stream::once(shutdown).map(|()| Ok(Run::Shutdown));
        let bootstrap = futures::stream::iter(disco.collect())
            .map(|(peer, addrs)| Ok(Run::Discovered { peer, addrs }));
        let gossip_events = self
            .gossip
            .subscribe()
            .await
            .map(|event| Ok(Run::Gossip { event }));

        info!("Listening on {:?}", endpoint.local_addr());

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
        .unwrap_or_else(|()| warn!("Shutting down"))
    }

    pub async fn subscribe(&self) -> impl futures::Stream<Item = ProtocolEvent> {
        self.subscribers.subscribe().await
    }

    pub async fn announce(&self, have: gossip::Update) {
        self.gossip.announce(have).await
    }

    pub async fn query(&self, want: gossip::Update) {
        self.gossip.query(want).await
    }

    pub async fn open_git(&self, to: &PeerId) -> Option<Stream> {
        trace!("Opening git stream to {}", to);
        if let Some(conn) = self.connections.lock().await.get(to) {
            conn.open_stream()
                .and_then(|stream| upgrade(stream, Upgrade::Git))
                .await
                .map_err(|e| error!("{}", e))
                .ok()
        } else {
            warn!("Error opening git stream: not connected to {}", to);
            None
        }
    }

    async fn eval_run(&mut self, endpoint: Endpoint, run: Run<'_>) -> Result<(), ()> {
        match run {
            Run::Discovered { peer, addrs } => {
                trace!("Run::Discovered: {}@{:?}", peer, addrs);
                if !self.connections.lock().await.contains_key(&peer) {
                    if let Some((conn, incoming)) = connect(&endpoint, &peer, addrs).await {
                        self.handle_connect(conn, incoming.boxed(), None).await;
                    }
                }

                Ok(())
            },

            Run::Incoming { conn, incoming } => {
                trace!("Run::Incoming: {}", conn.remote_address());
                self.handle_incoming(conn, incoming)
                    .await
                    .map_err(|e| warn!("Error processing incoming connection: {}", e))
            },

            Run::Gossip { event } => match event {
                gossip::ProtocolEvent::SendAdhoc(hello) => {
                    let gossip::Hello { to, rpc } = *hello;

                    trace!("Run::Rad(SendAdhoc): {}", to.peer_id);
                    let conn = match self.connections.lock().await.get(&to.peer_id) {
                        Some(conn) => Some(conn.clone()),
                        None => connect_peer_info(&endpoint, to).await.map(|(conn, _)| conn),
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

                gossip::ProtocolEvent::Connect(hello) => {
                    let gossip::Hello { to, rpc } = *hello;

                    trace!("Run::Rad(Connect): {}", to.peer_id);
                    if !self.connections.lock().await.contains_key(&to.peer_id) {
                        let conn = connect_peer_info(&endpoint, to).await;
                        if let Some((conn, incoming)) = conn {
                            self.handle_connect(conn, incoming.boxed(), Some(rpc)).await
                        }
                    }

                    Ok(())
                },

                gossip::ProtocolEvent::Disconnect(peer) => {
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
        mut incoming: BoxStream<'_, Result<Stream, Error>>,
        hello: impl Into<Option<gossip::Rpc>>,
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
                while let Some(stream) = incoming.try_next().await? {
                    this2.incoming(stream).await?
                }

                Ok(())
            }
        );

        if let Err(e) = res {
            warn!("Closing connection with {}, because: {}", remote_id, e);
            conn.close(CloseReason::InternalError);
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

    async fn handle_incoming<Incoming>(
        &self,
        conn: Connection,
        mut incoming: Incoming,
    ) -> Result<(), Error>
    where
        Incoming: futures::Stream<Item = Result<Stream, Error>> + Unpin,
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

        while let Some(stream) = incoming.try_next().await? {
            trace!("New incoming stream");
            let this = self.clone();
            tokio::spawn(async move {
                if let Err(e) = this.incoming(stream).await {
                    warn!("Incoming stream error: {}", e);
                }
            });
        }

        Ok(())
    }

    async fn outgoing(
        &mut self,
        stream: Stream,
        hello: impl Into<Option<gossip::Rpc>>,
    ) -> Result<(), Error> {
        let upgraded = upgrade(stream, Upgrade::Gossip).await?;
        self.gossip
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

                    trace!("Incoming stream upgraded to {:?}", upgrade);

                    // remove framing
                    let stream = stream.release().0;
                    match upgrade {
                        Upgrade::Gossip => self
                            .gossip
                            .incoming(stream.framed(CborCodec::new()))
                            .await
                            .map_err(|e| format_err!("Error handling gossip upgrade: {}", e)),

                        Upgrade::Git => self
                            .git
                            .invoke_service(stream.split())
                            .await
                            .map_err(|e| format_err!("Error handling git upgrade: {}", e)),
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
    S: gossip::LocalStorage + 'static,
{
    async fn open_stream(&self, to: &PeerId) -> Option<Stream> {
        self.open_git(to).await
    }
}

async fn connect_peer_info(
    endpoint: &Endpoint,
    peer_info: gossip::PeerInfo,
) -> Option<(
    Connection,
    impl futures::Stream<Item = Result<Stream, Error>> + Unpin,
)> {
    let addrs = iter::once(peer_info.advertised_info.listen_addr).chain(peer_info.seen_addrs);
    connect(endpoint, &peer_info.peer_id, addrs).await
}

async fn connect<I>(
    endpoint: &Endpoint,
    peer_id: &PeerId,
    addrs: I,
) -> Option<(
    Connection,
    impl futures::Stream<Item = Result<Stream, Error>> + Unpin,
)>
where
    I: IntoIterator<Item = SocketAddr>,
{
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
                        Upgrade::Gossip,
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
