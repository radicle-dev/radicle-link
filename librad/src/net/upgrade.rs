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

//! We support one-way protocol upgrades on individual QUIC streams
//! (irrespective of ALPN, which applies per-connection). This module implements
//! the negotiation protocol.

use std::{io, marker::PhantomData, ops::Deref, pin::Pin};

use futures::{
    future::{BoxFuture, FutureExt},
    io::{AsyncRead, AsyncWrite},
    sink::SinkExt,
    stream::TryStreamExt,
    task::{Context, Poll},
};
use futures_codec::{CborCodec, CborCodecError, Framed};
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use thiserror::Error;

use crate::{git::transport::GitStream, net::quic};

pub struct Gossip;
pub struct Git;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum UpgradeRequest {
    Gossip = 0,
    Git = 1,
}

impl Into<UpgradeRequest> for Gossip {
    fn into(self) -> UpgradeRequest {
        UpgradeRequest::Gossip
    }
}

impl Into<UpgradeRequest> for Git {
    fn into(self) -> UpgradeRequest {
        UpgradeRequest::Git
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpgradeResponse {
    // TODO(kim): Technically, we don't need a confirmation. Keeping it here for
    // now, so we can send back an error. Maybe we'll also need some additional
    // response payload in the future, who knows.
    SwitchingProtocols(UpgradeRequest),
    Error(UpgradeError),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpgradeError {
    InvalidPayload,
    UnsupportedUpgrade(UpgradeRequest), // reserved
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Protocol mismatch: expected {expected:?}, got {actual:?}")]
    ProtocolMismatch {
        expected: UpgradeRequest,
        actual: UpgradeRequest,
    },

    #[error("Remote peer denied upgrade: {0:?}")]
    ErrorResponse(UpgradeError),

    #[error("Local peer denied upgrade: {0:?}")]
    Denied(UpgradeRequest),

    #[error("No response from remote peer")]
    NoResponse,

    #[error(transparent)]
    Cbor(#[from] serde_cbor::Error),

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

pub struct Upgraded<S, U> {
    stream: S,
    _marker: PhantomData<U>,
}

impl<S, U> Upgraded<S, U> {
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            _marker: PhantomData,
        }
    }

    pub fn into_stream(self) -> S {
        self.stream
    }
}

impl<S, U> Deref for Upgraded<S, U> {
    type Target = S;

    fn deref(&self) -> &Self::Target {
        &self.stream
    }
}

impl<S, U> AsyncRead for Upgraded<S, U>
where
    S: AsyncRead + Unpin,
    U: Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        AsyncRead::poll_read(Pin::new(&mut self.get_mut().stream), cx, buf)
    }
}

impl<S, U> AsyncWrite for Upgraded<S, U>
where
    S: AsyncWrite + Unpin,
    U: Unpin,
{
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<io::Result<usize>> {
        AsyncWrite::poll_write(Pin::new(&mut self.get_mut().stream), cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        AsyncWrite::poll_flush(Pin::new(&mut self.get_mut().stream), cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        AsyncWrite::poll_close(Pin::new(&mut self.get_mut().stream), cx)
    }
}

impl<S> GitStream for Upgraded<S, Git> where S: AsyncRead + AsyncWrite + Unpin + Send + Sync {}

pub async fn upgrade<S, U>(stream: S, upgrade: U) -> Result<Upgraded<S, U>, Error>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + Sync,
    U: Into<UpgradeRequest>,
{
    let upgrade: UpgradeRequest = upgrade.into();

    let mut stream = Framed::new(stream, CborCodec::<UpgradeRequest, UpgradeResponse>::new());
    stream.send(upgrade).await?;
    match stream.try_next().await? {
        Some(resp) => match resp {
            UpgradeResponse::SwitchingProtocols(proto) => {
                if proto == upgrade {
                    Ok(Upgraded {
                        stream: stream.release().0,
                        _marker: PhantomData,
                    })
                } else {
                    Err(Error::ProtocolMismatch {
                        expected: upgrade,
                        actual: proto,
                    })
                }
            },
            UpgradeResponse::Error(e) => Err(Error::ErrorResponse(e)),
        },
        None => Err(Error::NoResponse),
    }
}

pub type SwitchingProtocols<'a, S, U> = BoxFuture<'a, Result<Upgraded<S, U>, Error>>;

pub enum WithUpgrade<'a, S> {
    Gossip(SwitchingProtocols<'a, S, Gossip>),
    Git(SwitchingProtocols<'a, S, Git>),
}

pub async fn with_upgrade<'a, S>(incoming: S) -> Result<WithUpgrade<'a, S>, Error>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + Sync + 'a,
{
    let mut incoming = Framed::new(
        incoming,
        CborCodec::<UpgradeResponse, UpgradeRequest>::new(),
    );
    match incoming.try_next().await {
        Ok(resp) => match resp {
            None => Err(Error::NoResponse),
            Some(resp) => {
                let switching = async move {
                    incoming
                        .send(UpgradeResponse::SwitchingProtocols(resp))
                        .await?;
                    Ok(incoming.release().0)
                };

                let upgrade = match resp {
                    UpgradeRequest::Gossip => {
                        WithUpgrade::Gossip(switching.map(|s| s.map(Upgraded::new)).boxed())
                    },
                    UpgradeRequest::Git => {
                        WithUpgrade::Git(switching.map(|s| s.map(Upgraded::new)).boxed())
                    },
                };

                Ok(upgrade)
            },
        },

        Err(e) => {
            let _ = incoming
                .send(UpgradeResponse::Error(UpgradeError::InvalidPayload))
                .await;
            Err(e.into())
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use anyhow::{anyhow, Error};
    use futures::try_join;
    use futures_await_test::async_test;

    use crate::{keys::SecretKey, net::connection::mock::MockStream};

    #[async_test]
    async fn test_upgrade() -> Result<(), Error> {
        let (initiator, receiver) =
            MockStream::pair(SecretKey::new().into(), SecretKey::new().into(), 512);

        try_join!(
            async { upgrade(initiator, Git).await.map_err(|e| e.into()) },
            async {
                match with_upgrade(receiver).await? {
                    WithUpgrade::Git(switching) => switching.await.map_err(|e| e.into()),
                    _ => Err(anyhow!("Wrong upgrade")),
                }
            }
        )
        .map(|_| ())
    }
}
