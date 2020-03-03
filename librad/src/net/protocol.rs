use std::{
    collections::HashMap,
    hash::Hash,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use failure::{format_err, Error};
use futures::{
    executor::block_on,
    future::TryFutureExt,
    sink::SinkExt,
    stream::{BoxStream, StreamExt, TryStreamExt},
};
use futures_codec::CborCodec;
use log::{error, warn};
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

use crate::{
    git::{server::GitServer, transport::GitStreamFactory},
    internal::channel::Fanout,
    net::{
        connection::{CloseReason, Connection, Endpoint, Stream},
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

#[derive(Clone)]
pub struct Protocol<A, S> {
    rad: rad::Protocol<A, S>,
    git: GitServer,

    connections: Arc<Mutex<HashMap<PeerId, Connection>>>,
    subscribers: Fanout<ProtocolEvent>,
}

impl<A, S> Protocol<A, S>
where
    for<'de> A: Clone + Eq + Send + Hash + Deserialize<'de> + Serialize + 'static,
{
    pub fn new(rad: rad::Protocol<A, S>, git: GitServer) -> Self {
        Self {
            rad,
            git,
            connections: Arc::new(Mutex::new(HashMap::default())),
            subscribers: Fanout::new(),
        }
    }

    pub async fn run<D>(
        &mut self,
        (endpoint, incoming): (
            Endpoint,
            impl futures::Stream<Item = (Connection, impl futures::Stream<Item = Stream> + Unpin)>,
        ),
        disco: D,
    ) where
        D: Discovery,
        S: rad::LocalStorage<A>,
    {
        enum Run<A, S> {
            Connect {
                conn: Connection,
                incoming: S,
                hello: Option<rad::Rpc<A>>,
            },
            Disconnect {
                peer: PeerId,
            },
        }

        let incoming1: dyn futures::Stream<Item = Run<A, S>> + Sized + Unpin =
            incoming.filter_map(|(conn, i)| async move {
                Some(Run::Connect {
                    conn,
                    incoming: i,
                    hello: None,
                })
            });

        let bootstrap: dyn futures::Stream<Item = Run<A, S>> + Sized + Unpin =
            futures::stream::iter(disco.collect()).filter_map(|(peer_id, addrs)| {
                let endpoint = endpoint.clone();
                async move {
                    Self::try_connect(&endpoint, &peer_id, &addrs)
                        .await
                        .map(|(conn, incoming)| Run::Connect {
                            conn,
                            incoming,
                            hello: None,
                        })
                }
            });

        let rad_events: dyn futures::Stream<Item = Run<A, S>> + Sized + Unpin =
            self.rad.subscribe().filter_map(|evt| {
                let endpoint = endpoint.clone();
                async move {
                    match evt {
                        rad::ProtocolEvent::DialAndSend(peer_info, rpc) => Self::try_connect(
                            &endpoint,
                            &peer_info.peer_id,
                            &peer_info
                                .seen_addrs
                                .iter()
                                .cloned()
                                .collect::<Vec<SocketAddr>>(),
                        )
                        .await
                        .map(|(conn, incoming)| Run::Connect {
                            conn,
                            incoming,
                            hello: Some(rpc),
                        }),

                        rad::ProtocolEvent::Disconnect(peer) => Some(Run::Disconnect { peer }),
                    }
                }
            });

        futures::stream::select_all(&[incoming1, rad_events, bootstrap])
            .for_each_concurrent(/* limit */ None, |run| {
                let mut this = self.clone();
                async move {
                    match run {
                        Run::Connect {
                            conn,
                            incoming,
                            hello,
                        } => {
                            this.subscribers
                                .emit(ProtocolEvent::Connected(conn.peer_id().clone()))
                                .await;
                            this.drive_connection(conn, incoming, hello).await
                        },

                        Run::Disconnect { peer } => {
                            if let Some(conn) = this.connections.lock().unwrap().remove(&peer) {
                                conn.close(CloseReason::ProtocolDisconnect);
                                this.subscribers
                                    .emit(ProtocolEvent::Disconnected(peer))
                                    .await
                            }
                        },
                    }
                }
            })
            .await
    }

    pub fn subscribe(&self) -> impl futures::Stream<Item = ProtocolEvent> {
        self.subscribers.subscribe()
    }

    pub async fn announce(&self, have: A) {
        self.rad.announce(have).await
    }

    pub async fn query(&self, want: A) {
        self.rad.query(want).await
    }

    pub async fn open_git(&self, to: &PeerId) -> Option<Stream> {
        async {
            if let Some(conn) = self.connections.lock().unwrap().get(to) {
                conn.open_stream()
                    .and_then(|stream| Self::upgrade(stream, Upgrade::Git))
                    .await
                    .ok()
            } else {
                None
            }
        }
        .await
    }

    async fn try_connect(
        endpoint: &Endpoint,
        peer_id: &PeerId,
        addrs: &[SocketAddr],
    ) -> Option<(Connection, impl futures::Stream<Item = Stream> + Unpin)> {
        futures::stream::iter(addrs)
            .filter_map(|addr| {
                let mut endpoint = endpoint.clone();
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

    async fn drive_connection(
        &mut self,
        conn: Connection,
        mut incoming: impl futures::Stream<Item = Stream> + Unpin,
        outgoing_hello: impl Into<Option<rad::Rpc<A>>>,
    ) where
        S: rad::LocalStorage<A>,
    {
        let mut this1 = self.clone();
        let this2 = self.clone();

        futures::try_join!(
            async {
                let outgoing = conn.open_stream().await?;
                this1.outgoing(outgoing, outgoing_hello).await
            },
            async {
                while let Some(stream) = incoming.next().await {
                    this2.incoming(stream).await?
                }
                Ok(())
            }
        )
        .map(|_| ())
        .unwrap_or_else(|e| {
            error!("Closing connection with {}, because: {}", conn.peer_id(), e);
            conn.close(CloseReason::ConnectionError);
        })
    }

    async fn outgoing(
        &mut self,
        stream: Stream,
        hello: impl Into<Option<rad::Rpc<A>>>,
    ) -> Result<(), Error>
    where
        S: rad::LocalStorage<A>,
    {
        let upgraded = Self::upgrade(stream, Upgrade::Rad).await?;
        self.rad
            .outgoing(upgraded.framed(CborCodec::new()), hello)
            .await
            .map_err(|e| e.into())
    }

    async fn upgrade(stream: Stream, upgrade: Upgrade) -> Result<Stream, Error> {
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
                Some(UpgradeResponse::Error(e)) => {
                    Err(format_err!("Peer denied rad upgrade: {:?}", e))
                },
                None => Err(format_err!("Silent server")),
            },

            Err(e) => Err(format_err!("Error deserialising upgrade response: {:?}", e)),
        }
    }

    async fn incoming(&self, stream: Stream) -> Result<(), Error>
    where
        S: rad::LocalStorage<A>,
    {
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
                        Upgrade::Rad => self
                            .rad
                            .incoming(stream.framed(CborCodec::new()))
                            .await
                            .map_err(|e| format_err!("Error handling rad upgrade: {}", e)),

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

impl<A, S> GitStreamFactory for Protocol<A, S>
where
    for<'de> A: Clone + Eq + Hash + Send + Deserialize<'de> + Serialize + 'static,
    S: Send + Sync,
{
    fn open_stream(&self, to: &PeerId) -> Option<Stream> {
        block_on(self.open_git(to))
    }
}
