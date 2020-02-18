use futures::{sink::SinkExt, stream::TryStreamExt, AsyncRead, AsyncWrite};
use futures_codec::{CborCodec, FramedRead, FramedWrite};
use log::error;
use serde::{Deserialize, Serialize};

use crate::git::server::GitServer;

pub mod rad;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Upgrade {
    Rad = 0, // reserved
    Git = 1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpgradeError {
    InvalidPayload,
    UnsupportedUpgrade(Upgrade), // reserved
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientHello {
    upgrade: Upgrade,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerHello {
    StreamUpgradeOk,
    StreamUpgradeErr(UpgradeError),
}

pub struct Protocol {
    rad: rad::Protocol,
    git: GitServer,
}

impl Protocol {
    pub fn new(rad: rad::Protocol, git: GitServer) -> Self {
        Self { rad, git }
    }

    pub async fn handle_incoming<R, W>(&self, recv: R, send: W)
    where
        R: AsyncRead + tokio::io::AsyncRead + Unpin,
        W: AsyncWrite + tokio::io::AsyncWrite + Unpin,
    {
        let codec = CborCodec::<ServerHello, ClientHello>::new();
        let mut framed_recv = FramedRead::new(recv, codec.clone());
        let mut framed_send = FramedWrite::new(send, codec);

        match framed_recv.try_next().await {
            Ok(Some(ClientHello { upgrade })) => {
                if framed_send.send(ServerHello::StreamUpgradeOk).await.is_ok() {
                    // remove framing
                    let recv = framed_recv.release().0;
                    let send = framed_send.release().0;

                    match upgrade {
                        Upgrade::Rad => self
                            .rad
                            .handle_incoming(recv, send)
                            .await
                            .unwrap_or_else(|e| error!("Error handling rad upgrade: {}", e)),

                        Upgrade::Git => self
                            .git
                            .invoke_service(recv, send)
                            .await
                            .unwrap_or_else(|e| error!("Error handling git upgrade: {}", e)),
                    }
                }
            },

            Ok(None) => error!("Silent client"),
            Err(e) => {
                error!("Error deserialising client hello: {:?}", e);
                let _ = framed_send
                    .send(ServerHello::StreamUpgradeErr(UpgradeError::InvalidPayload))
                    .await;
            },
        }
    }
}
