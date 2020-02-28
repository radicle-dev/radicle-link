use std::net::SocketAddr;

use failure::{format_err, Error};
use futures::{
    sink::SinkExt,
    stream::{StreamExt, TryStreamExt},
};
use futures_codec::{CborCodec, Framed};
use log::{error, warn};
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

use crate::{
    git::server::GitServer,
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
        let bootstrap = futures::stream::iter(disco.collect()).filter_map(|(peer_id, addrs)| {
            let endpoint = endpoint.clone();
            async move {
                Self::try_connect(&endpoint, &peer_id, &addrs)
                    .await
                    .map(|conn| (conn, None))
            }
        });

        let rad_events =
            self.rad
                .subscribe()
                .filter_map(|rad::ProtocolEvent::DialAndSend(peer_info, rpc)| {
                    let endpoint = endpoint.clone();
                    async move {
                        Self::try_connect(
                            &endpoint,
                            &peer_info.peer_id,
                            &peer_info
                                .seen_addrs
                                .iter()
                                .cloned()
                                .collect::<Vec<SocketAddr>>(),
                        )
                        .await
                        .map(|conn| (conn, Some(rpc)))
                    }
                });

        futures::stream::select(rad_events, bootstrap)
            .for_each_concurrent(/* limit */ None, |((conn, incoming), hello)| {
                let mut this = self.clone();
                async move { this.drive_connection(conn, incoming, hello).await }
            })
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
        outgoing_hello: impl Into<Option<rad::Rpc>>,
    ) {
        let mut this1 = self.clone();
        let this2 = self.clone();

        futures::try_join!(this1.outgoing(conn.clone(), outgoing_hello), async {
            while let Some(stream) = incoming.next().await {
                this2.incoming(stream).await?
            }
            Ok(())
        })
        .map(|_| ())
        .unwrap_or_else(|e| {
            error!("Closing connection with {}, because: {}", conn.peer_id(), e);
            conn.close(CloseReason::ConnectionError)
        })
    }

    async fn outgoing(
        &mut self,
        conn: Connection,
        hello: impl Into<Option<rad::Rpc>>,
    ) -> Result<(), Error> {
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
                    .outgoing(Framed::new(stream.release().0, CborCodec::new()), hello)
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
