use std::{
    collections::HashMap,
    hash::Hash,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use failure::{format_err, Error};
use futures::{
    future::TryFutureExt,
    sink::SinkExt,
    stream::{StreamExt, TryStreamExt},
    AsyncRead,
    AsyncWrite,
};
use futures_codec::CborCodec;
use log::{error, warn};
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

use crate::{
    git::server::GitServer,
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
pub struct Protocol<A: Clone + Eq + Hash, S> {
    rad: rad::Protocol<A, S>,
    git: GitServer,

    connections: Arc<Mutex<HashMap<PeerId, Connection>>>,
    subscribers: Fanout<ProtocolEvent>,
}

impl<A, S> Protocol<A, S>
where
    for<'de> A: Clone + Eq + Hash + Deserialize<'de> + Serialize + 'static,
    S: rad::LocalStorage<A>,
{
    pub fn new(rad: rad::Protocol<A, S>, git: GitServer) -> Self {
        Self {
            rad,
            git,
            connections: Arc::new(Mutex::new(HashMap::default())),
            subscribers: Fanout::new(),
        }
    }

    pub async fn run<D: Discovery>(&mut self, endpoint: Endpoint, disco: D) {
        enum Run<A, S> {
            Outgoing {
                conn: Connection,
                incoming: S,
                hello: Option<rad::Rpc<A>>,
            },
            Disconnect {
                peer: PeerId,
            },
        }

        let bootstrap = futures::stream::iter(disco.collect()).filter_map(|(peer_id, addrs)| {
            let endpoint = endpoint.clone();
            async move {
                Self::try_connect(&endpoint, &peer_id, &addrs)
                    .await
                    .map(|(conn, incoming)| Run::Outgoing {
                        conn,
                        incoming,
                        hello: None,
                    })
            }
        });

        let rad_events = self.rad.subscribe().filter_map(|evt| {
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
                    .map(|(conn, incoming)| Run::Outgoing {
                        conn,
                        incoming,
                        hello: Some(rpc),
                    }),

                    rad::ProtocolEvent::Disconnect(peer) => Some(Run::Disconnect { peer }),
                }
            }
        });

        futures::stream::select(rad_events, bootstrap)
            .for_each_concurrent(/* limit */ None, |run| {
                let mut this = self.clone();
                async move {
                    match run {
                        Run::Outgoing {
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

    pub async fn open_git(&self, to: &PeerId) -> Option<impl AsyncRead + AsyncWrite + Unpin> {
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
    ) {
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
    ) -> Result<(), Error> {
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
                        Upgrade::Rad => self
                            .rad
                            .incoming(stream.framed(CborCodec::new()))
                            .await
                            .map_err(|e| format_err!("Error handling rad upgrade: {}", e)),

                        Upgrade::Git => self
                            .git
                            .invoke_service(stream.into())
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
