// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! This module implements the wire format of the RPC protocol. As specified in
//! docs/rfc/0696-p2p-node.adoc
//!
//! The structure of messages in this protocol is as followed:
//!
//! ```svgbob
//! .--------------------------------------.
//! | 4 byte big endian length prefix      |
//! +--------------------------------------+
//! | CBOR encoding of the message headers |
//! +--------------------------------------+
//! | Payload (arbitrary bytes)            |
//! `--------------------------------------'
//! ```
//!
//! In practice the request payload is also a CBOR object of some kind.
//!
//! To read these messages then you first decode the length prefix. Then decode
//! the request headers (which corresponds to either the `RequestHeaders` or
//! `ResponseHeaders` types herein) and then the remaining length (which may be
//! zero) is the payload.
//!
//! The headers of the message contain a `kind` enum (either `RequestKind` or
//! `ResponseKind`) which implementations should read to determine how the
//! payload should be handled.

use super::messages;

pub(crate) mod request;
pub(crate) use request::Request;
pub(crate) mod response;
pub(crate) use response::Response;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Message<Headers> {
    pub headers: Headers,
    pub payload: Option<Vec<u8>>,
}

#[derive(Clone, Debug, minicbor::Decode, minicbor::Encode, PartialEq)]
#[cbor(transparent)]
pub struct UserAgent(#[n(0)] String);

impl From<messages::UserAgent> for UserAgent {
    fn from(ua: messages::UserAgent) -> Self {
        let s: &str = (&ua).into();
        UserAgent(s.to_string())
    }
}

impl From<UserAgent> for messages::UserAgent {
    fn from(s: UserAgent) -> Self {
        s.0.into()
    }
}

impl From<&str> for UserAgent {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, minicbor::Decode, minicbor::Encode,
)]
#[cbor(transparent)]
pub struct RequestId(#[n(0)] minicbor::bytes::ByteVec);

impl From<messages::RequestId> for RequestId {
    fn from(r: messages::RequestId) -> Self {
        Self(Vec::<u8>::from(r).into())
    }
}

impl From<RequestId> for messages::RequestId {
    fn from(r: RequestId) -> Self {
        let raw: Vec<u8> = r.0.into();
        raw.into()
    }
}

impl AsRef<[u8]> for RequestId {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<Vec<u8>> for RequestId {
    fn from(bytes: Vec<u8>) -> Self {
        Self(bytes.into())
    }
}

#[derive(Debug, Clone, minicbor::Encode, minicbor::Decode, PartialEq)]
#[cbor(transparent)]
pub struct Progress(#[n(0)] String);

impl From<String> for Progress {
    fn from(s: String) -> Self {
        Self(s)
    }
}

#[derive(Debug, Clone, minicbor::Encode, minicbor::Decode, PartialEq)]
#[cbor(transparent)]
pub struct Error(#[n(0)] String);

impl From<String> for Error {
    fn from(s: String) -> Self {
        Self(s)
    }
}
