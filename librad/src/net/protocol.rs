use failure::{format_err, Error};
use futures::{
    sink::SinkExt,
    stream::{StreamExt, TryStreamExt},
};
use futures_codec::CborCodec;
use log::{error, warn};
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

use crate::{
    git::server::GitServer,
    net::{
        connection::{CloseReason, Connection, Endpoint, IncomingStreams, Stream},
        discovery::Discovery,
    },
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
    // configuration value in the response. Let's see.
    SwitchingProtocols(Upgrade),
    Error(UpgradeError),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpgradeError {
    InvalidPayload,
    UnsupportedUpgrade(Upgrade), // reserved
}

#[derive(Clone)]
pub struct Protocol {
    rad: rad::Protocol,
    git: GitServer,
}

impl Protocol {
    pub fn new(rad: rad::Protocol, git: GitServer) -> Self {
        Self { rad, git }
    }

    pub async fn run<D: Discovery>(&mut self, endpoint: Endpoint, disco: D) {
        // attempt to connect
        let peers: Vec<(Connection, IncomingStreams)> = futures::stream::iter(disco.collect())
            .filter_map(|(peer_id, addrs)| {
                let mut ep = endpoint.clone();
                async move {
                    for addr in addrs {
                        match ep.connect(&peer_id, &addr).await {
                            Ok(conn) => return Some(conn),
                            Err(e) => warn!("Could not connect to {} at {}: {}", peer_id, addr, e),
                        }
                    }
                    None
                }
            })
            .collect()
            .await;

        if peers.is_empty() {
            warn!("No connection to a seed node could be established!");
        }

        futures::stream::iter(peers)
            .for_each_concurrent(/* limit */ 32, |(conn, mut incoming)| {
                let mut self1 = self.clone();
                let self2 = self.clone();
                async move {
                    futures::try_join!(self1.outgoing(conn.clone()), async {
                        while let Some((send, recv)) = incoming.try_next().await? {
                            self2
                                .incoming(Stream::new(conn.peer_id().clone(), recv, send))
                                .await?
                        }
                        Ok(())
                    })
                    .map(|_| ())
                    .unwrap_or_else(|e| {
                        error!(
                            "Closing outgoing connection to {}, because: {}",
                            conn.peer_id(),
                            e
                        );
                        conn.close(CloseReason::ConnectionError)
                    })
                }
            })
            .await
    }

    async fn outgoing(&mut self, conn: Connection) -> Result<(), Error> {
        let mut stream = conn
            .open_stream()
            .await?
            .framed(CborCodec::<Upgrade, UpgradeResponse>::new());

        stream
            .send(Upgrade::Rad)
            .await
            .map_err(|e| format_err!("Failed to send rad upgrade: {:?}", e))?;

        match stream.try_next().await {
            Ok(resp) => match resp {
                Some(UpgradeResponse::SwitchingProtocols(Upgrade::Rad)) => self
                    .rad
                    .outgoing(conn, stream.release().0)
                    .await
                    .map_err(|e| e.into()),

                Some(UpgradeResponse::SwitchingProtocols(upgrade)) => Err(format_err!(
                    "Protocol mismatch: requested {:?}, got {:?}",
                    Upgrade::Rad,
                    upgrade
                )),
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
                            .incoming(stream)
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
