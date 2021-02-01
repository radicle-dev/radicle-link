// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! We support one-way protocol upgrades on individual QUIC streams
//! (irrespective of ALPN, which applies per-connection). This module implements
//! the negotiation protocol.

use std::{
    fmt::{self, Debug, Display},
    io,
    marker::PhantomData,
    ops::Deref,
    pin::Pin,
    time::Duration,
};

use futures::{
    future::{self, TryFutureExt as _},
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    task::{Context, Poll},
};
use futures_timer::Delay;
use thiserror::Error;

use crate::git::p2p::transport::GitStream;

/// Timeout waiting for an [`UpgradeRequest`].
// NOTE: This is a magic constant, which should account for very slow
// links. May need to become a protocol config parameter if we see very busy
// nodes time out a lot.
const RECV_UPGRADE_TIMEOUT: Duration = Duration::from_secs(23);

/// Length in bytes of the CBOR encoding of [`UpgradeRequest`].
///
/// We use this to allocate only a fixed-size buffer, and not deal with
/// unconsumed bytes.
// NOTE: Make sure to adjust in case [`UpgradeRequest`] gains larger variants.
const UPGRADE_REQUEST_ENCODING_LEN: usize = 4;

#[derive(Debug)]
pub struct Gossip;

#[derive(Debug)]
pub struct Git;

#[derive(Debug)]
pub struct Membership;

#[derive(Debug)]
pub struct Graft;

/// Signal the (sub-) protocol about to be sent over a given QUIC stream.
///
/// This is only valid as the first message sent by the initiator of a fresh
/// stream. No response is to be expected, the initiator may start sending data
/// immediately after. If the receiver is not able or willing to handle the
/// protocol upgrade, it shall simply close the stream.
///
/// # Wire Encoding
///
/// The message is encoded as a 2-element CBOR array, where the first element is
/// the (major) version tag (currently `0` (zero)). The second element is of
/// CBOR major type 0 (unsigned integer), with the value being the `u8`
/// discriminator of the enum. This allows _compatible_ changes to
/// [`UpgradeRequest`] (ie. both ends can handle the absence of a variant), as
/// well as _incompatible_ evolution by incrementing the version tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UpgradeRequest {
    Gossip = 0,
    Git = 1,
    Membership = 2,
    Graft = 3,
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

impl Into<UpgradeRequest> for Membership {
    fn into(self) -> UpgradeRequest {
        UpgradeRequest::Membership
    }
}

impl Into<UpgradeRequest> for Graft {
    fn into(self) -> UpgradeRequest {
        UpgradeRequest::Graft
    }
}

impl minicbor::Encode for UpgradeRequest {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.array(2)?.u8(0)?.u8(*self as u8)?.end()?;
        Ok(())
    }
}

impl<'de> minicbor::Decode<'de> for UpgradeRequest {
    fn decode(d: &mut minicbor::Decoder<'de>) -> Result<Self, minicbor::decode::Error> {
        if Some(2) != d.array()? {
            return Err(minicbor::decode::Error::Message("expected 2-element array"));
        }

        match d.u8()? {
            0 => match d.u8()? {
                0 => Ok(Self::Gossip),
                1 => Ok(Self::Git),
                2 => Ok(Self::Membership),
                3 => Ok(Self::Graft),
                n => Err(minicbor::decode::Error::UnknownVariant(n as u32)),
            },
            n => Err(minicbor::decode::Error::UnknownVariant(n as u32)),
        }
    }
}

#[derive(Error)]
#[error("stream upgrade failed")]
pub struct Error<S> {
    pub stream: S,
    pub source: ErrorSource,
}

impl<S> Debug for Error<S> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(self, f)
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ErrorSource {
    #[error("timed out")]
    Timeout,

    #[error(transparent)]
    Encode(#[from] minicbor::encode::Error<io::Error>),

    #[error(transparent)]
    Decode(#[from] minicbor::decode::Error),

    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(Debug)]
pub struct Upgraded<U, S> {
    stream: S,
    _marker: PhantomData<U>,
}

impl<U, S> Upgraded<U, S> {
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            _marker: PhantomData,
        }
    }

    pub fn into_stream(self) -> S {
        self.stream
    }

    pub fn map<F, T>(self, f: F) -> Upgraded<U, T>
    where
        F: FnOnce(S) -> T,
    {
        Upgraded {
            stream: f(self.stream),
            _marker: PhantomData,
        }
    }
}

impl<U, S> Deref for Upgraded<U, S> {
    type Target = S;

    fn deref(&self) -> &Self::Target {
        &self.stream
    }
}

impl<U, S> AsyncRead for Upgraded<U, S>
where
    U: Unpin,
    S: AsyncRead + Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        AsyncRead::poll_read(Pin::new(&mut self.get_mut().stream), cx, buf)
    }
}

impl<U, S> AsyncWrite for Upgraded<U, S>
where
    U: Unpin,
    S: AsyncWrite + Unpin,
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

impl<S> GitStream for Upgraded<Git, S> where S: AsyncRead + AsyncWrite + Unpin + Send + Sync {}

#[derive(Debug)]
pub enum SomeUpgraded<S> {
    Gossip(Upgraded<Gossip, S>),
    Git(Upgraded<Git, S>),
    Membership(Upgraded<Membership, S>),
    Graft(Upgraded<Graft, S>),
}

impl<S> SomeUpgraded<S> {
    pub fn map<F, T>(self, f: F) -> SomeUpgraded<T>
    where
        F: FnOnce(S) -> T,
    {
        match self {
            Self::Gossip(up) => SomeUpgraded::Gossip(up.map(f)),
            Self::Git(up) => SomeUpgraded::Git(up.map(f)),
            Self::Membership(up) => SomeUpgraded::Membership(up.map(f)),
            Self::Graft(up) => SomeUpgraded::Graft(up.map(f)),
        }
    }
}

pub async fn upgrade<U, S>(mut stream: S, upgrade: U) -> Result<Upgraded<U, S>, Error<S>>
where
    U: Into<UpgradeRequest>,
    S: AsyncWrite + Unpin + Send + Sync,
{
    let send = async {
        let cbor = minicbor::to_vec(&upgrade.into())?;
        Ok(stream.write_all(&cbor).await?)
    };

    match send.await {
        Err(source) => Err(Error { stream, source }),
        Ok(()) => Ok(Upgraded::new(stream)),
    }
}

pub async fn with_upgraded<'a, S>(mut incoming: S) -> Result<SomeUpgraded<S>, Error<S>>
where
    S: AsyncRead + Unpin + Send + Sync + 'a,
{
    let recv = async {
        let mut buf = [0u8; UPGRADE_REQUEST_ENCODING_LEN];
        {
            let timeout = async {
                Delay::new(RECV_UPGRADE_TIMEOUT).await;
                Err(ErrorSource::Timeout)
            };
            let recv = async { Ok(incoming.read_exact(&mut buf).await?) };

            futures::pin_mut!(timeout);
            futures::pin_mut!(recv);

            future::try_select(timeout, recv)
                .map_ok(|ok| future::Either::factor_first(ok).0)
                .map_err(|er| future::Either::factor_first(er).0)
                .await?;
        }

        Ok(minicbor::decode(&buf)?)
    };

    match recv.await {
        Err(source) => Err(Error {
            stream: incoming,
            source,
        }),
        Ok(req) => {
            let upgrade = match req {
                UpgradeRequest::Gossip => SomeUpgraded::Gossip(Upgraded::new(incoming)),
                UpgradeRequest::Git => SomeUpgraded::Git(Upgraded::new(incoming)),
                UpgradeRequest::Membership => SomeUpgraded::Membership(Upgraded::new(incoming)),
                UpgradeRequest::Graft => SomeUpgraded::Graft(Upgraded::new(incoming)),
            };

            Ok(upgrade)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use futures::try_join;

    use crate::{keys::SecretKey, net::connection::mock::MockStream, peer::PeerId};
    use librad_test::roundtrip::*;

    lazy_static! {
        static ref INITIATOR: PeerId = PeerId::from(SecretKey::from_seed([
            164, 74, 212, 59, 165, 115, 21, 231, 172, 182, 132, 97, 153, 209, 157, 239, 159, 129,
            46, 66, 173, 231, 36, 196, 164, 59, 203, 197, 153, 232, 150, 24
        ]));
        static ref RECEIVER: PeerId = PeerId::from(SecretKey::from_seed([
            187, 77, 103, 158, 241, 220, 26, 209, 116, 9, 70, 140, 27, 149, 254, 144, 80, 207, 112,
            171, 189, 222, 235, 233, 211, 249, 4, 159, 219, 39, 166, 112
        ]));
    }

    async fn test_upgrade(
        req: impl Into<UpgradeRequest>,
    ) -> Result<SomeUpgraded<()>, Error<MockStream>> {
        let (initiator, receiver) = MockStream::pair(*INITIATOR, *RECEIVER, 512);
        try_join!(
            async { upgrade(initiator, req).await.map_err(Error::from) },
            async {
                with_upgraded(receiver)
                    .await
                    .map(|upgrade| upgrade.map(|_| ()))
            }
        )
        .map(|(_, upgrade)| upgrade)
    }

    #[async_test]
    async fn upgrade_gossip() {
        assert_matches!(test_upgrade(Git).await, Ok(SomeUpgraded::Git(_)))
    }

    #[async_test]
    async fn upgrade_git() {
        assert_matches!(test_upgrade(Gossip).await, Ok(SomeUpgraded::Gossip(_)))
    }

    #[async_test]
    async fn upgrade_membership() {
        assert_matches!(
            test_upgrade(Membership).await,
            Ok(SomeUpgraded::Membership(_))
        )
    }

    #[test]
    fn roundtrip_upgrade_request() {
        cbor_roundtrip(UpgradeRequest::Gossip);
        cbor_roundtrip(UpgradeRequest::Git);
        cbor_roundtrip(UpgradeRequest::Membership);
    }
}
