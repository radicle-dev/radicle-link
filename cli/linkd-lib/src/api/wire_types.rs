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

pub mod request;
pub use request::Request;
pub mod response;
pub use response::Response;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message<Headers> {
    pub headers: Headers,
    pub payload: Option<Vec<u8>>,
}

#[derive(Debug, Clone, minicbor::Encode, minicbor::Decode, PartialEq, Eq)]
#[cbor(transparent)]
pub struct Progress(#[n(0)] String);

impl From<String> for Progress {
    fn from(s: String) -> Self {
        Self(s)
    }
}

#[derive(Debug, Clone, minicbor::Encode, minicbor::Decode, PartialEq, Eq)]
#[cbor(transparent)]
pub struct Error(#[n(0)] String);

impl From<String> for Error {
    fn from(s: String) -> Self {
        Self(s)
    }
}
