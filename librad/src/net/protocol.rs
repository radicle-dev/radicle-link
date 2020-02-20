use failure::{format_err, Error};
use futures::{
    sink::SinkExt,
    stream::{StreamExt, TryStreamExt},
};
use futures_codec::CborCodec;
use log::{error, warn};
use serde::{Deserialize, Serialize};

use crate::{
    git::server::GitServer,
    net::{
        connection::{CloseReason, Connection, Stream},
        discovery::Discovery,
        membership::Membership,
    },
};

pub mod rad;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Upgrade {
    Rad = 0,
    Git = 1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpgradeResponse {
    Ok,
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
    membership: Membership,
}

impl Protocol {
    pub fn new(rad: rad::Protocol, git: GitServer, membership: Membership) -> Self {
        Self {
            rad,
            git,
            membership,
        }
    }

    pub async fn run<D: Discovery>(&mut self, disco: D) {
        // attempt to connect
        let peers: Vec<(Connection, quinn::IncomingBiStreams)> =
            futures::stream::iter(disco.collect())
                .filter_map(|(peer_id, addrs)| {
                    let mut membership = self.membership.clone();
                    async move {
                        for addr in addrs {
                            match membership.connect(&peer_id, &addr).await {
                                Ok(conn) => return Some(conn),
                                Err(e) => {
                                    warn!("Could not connect to {} at {}: {}", peer_id, addr, e)
                                },
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
                let this = self.clone();
                async move {
                    futures::try_join!(this.outgoing(conn.clone()), async {
                        while let Some(stream) = incoming.try_next().await? {
                            this.incoming(stream.into()).await?
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

    async fn outgoing(&self, conn: Connection) -> Result<(), Error> {
        let mut stream = conn
            .open_stream()
            .await?
            .framed(CborCodec::<Upgrade, UpgradeResponse>::new());

        stream
            .send(Upgrade::Rad)
            .await
            .map_err(|e| format_err!("Failed to send rad upgrade: {:?}", e))?;

        match stream.try_next().await {
            Ok(Some(UpgradeResponse::Ok)) => self
                .rad
                .outgoing(stream.release().0)
                .await
                .map_err(|e| e.into()),

            Ok(Some(UpgradeResponse::Error(e))) => {
                Err(format_err!("Peer denied rad upgrade: {:?}", e))
            },
            Ok(None) => Err(format_err!("Silent server")),
            Err(e) => Err(format_err!("Error deserialising upgrade response: {:?}", e)),
        }
    }

    async fn incoming(&self, stream: Stream) -> Result<(), Error> {
        let mut stream = stream.framed(CborCodec::<UpgradeResponse, Upgrade>::new());
        match stream.try_next().await {
            Ok(Some(upgrade)) => {
                stream
                    .send(UpgradeResponse::Ok)
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

            Ok(None) => Err(format_err!("Silent client")),
            Err(e) => {
                let _ = stream
                    .send(UpgradeResponse::Error(UpgradeError::InvalidPayload))
                    .await;
                Err(format_err!("Error deserialising upgrade request: {:?}", e))
            },
        }
    }
}
