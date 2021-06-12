// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::borrow::Cow;

use super::PeerAdvertisement;
use crate::identities::xor;

#[derive(Clone, Copy, Debug, minicbor::Encode, minicbor::Decode)]
pub enum Request {
    /// Request the remote peer's [`PeerAdvertisement`]
    #[n(0)]
    #[cbor(array)]
    GetAdvertisement,

    /// Ask the remote peer to tell us the network address it sees us as.
    #[n(1)]
    #[cbor(array)]
    EchoAddr,

    /// Request the complete list of URNs the remote peer has.
    ///
    /// The response is a compact representation with approximate membership
    /// tests, see [`xor::Xor`].
    #[n(2)]
    #[cbor(array)]
    GetUrns,
}

#[derive(minicbor::Encode, minicbor::Decode)]
pub enum Response<'a, Addr>
where
    Addr: Clone + Ord,
{
    /// An application-level error occurred, which prevented the responder from
    /// fulfilling the request.
    #[n(0)]
    #[cbor(array)]
    Error(#[n(0)] Error),

    /// Response to a [`Request::GetAdvertisement`].
    #[n(1)]
    #[cbor(array)]
    Advertisement(#[n(0)] PeerAdvertisement<Addr>),

    /// Response to a [`Request::EchoAddr`].
    #[n(2)]
    #[cbor(array)]
    YourAddr(#[n(0)] Addr),

    /// Response to a [`Request::GetUrns`].
    ///
    /// See [`xor::Xor`].
    #[n(3)]
    #[cbor(array)]
    Urns(#[n(0)] Cow<'a, xor::Xor>),
}

/// Error response.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum Error {
    /// Some unspecified internal error occurred.
    ///
    /// The requester may try again at a later time, but an immediate retry is
    /// more likely to fail again with the same error.
    Internal,

    /// The responder is busy with something.
    ///
    /// A retry after a small timeout is acceptable.
    TemporarilyUnavailable,

    /// Catch-all for unknown error codes (forwards-compatibility).
    ///
    /// This is for decoding, **do not** construct this variant.
    Unknown(u8),
}

impl Error {
    pub fn code(&self) -> u8 {
        match self {
            Error::Internal => 0,
            Error::TemporarilyUnavailable => 1,
            Error::Unknown(n) => *n,
        }
    }
}

impl From<u8> for Error {
    fn from(n: u8) -> Self {
        match n {
            0 => Self::Internal,
            1 => Self::TemporarilyUnavailable,
            x => Self::Unknown(x),
        }
    }
}

impl minicbor::Encode for Error {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.u8(self.code())?;
        Ok(())
    }
}

impl<'b> minicbor::Decode<'b> for Error {
    fn decode(d: &mut minicbor::Decoder<'b>) -> Result<Self, minicbor::decode::Error> {
        d.u8().map(Self::from)
    }
}
