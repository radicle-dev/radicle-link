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
pub struct Interrogation;

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
    Interrogation = 3,
}

impl From<Gossip> for UpgradeRequest {
    fn from(_gossip: Gossip) -> Self {
        UpgradeRequest::Gossip
    }
}

impl From<Git> for UpgradeRequest {
    fn from(_git: Git) -> Self {
        UpgradeRequest::Git
    }
}

impl From<Membership> for UpgradeRequest {
    fn from(_membership: Membership) -> Self {
        UpgradeRequest::Membership
    }
}

impl From<Interrogation> for UpgradeRequest {
    fn from(_interrogation: Interrogation) -> Self {
        UpgradeRequest::Interrogation
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
                3 => Ok(Self::Interrogation),
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
    Interrogation(Upgraded<Interrogation, S>),
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
            Self::Interrogation(up) => SomeUpgraded::Interrogation(up.map(f)),
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
                UpgradeRequest::Interrogation => {
                    SomeUpgraded::Interrogation(Upgraded::new(incoming))
                },
            };

            Ok(upgrade)
        },
    }
}
