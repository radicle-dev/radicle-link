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

use std::convert::{TryFrom, TryInto};

use super::messages;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Message<Headers> {
    pub headers: Headers,
    pub payload: Option<Vec<u8>>,
}

pub(crate) type Request = Message<RequestHeaders>;
pub(crate) type Response = Message<ResponseHeaders>;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RequestMode {
    FireAndForget,
    ReportProgress,
    Unknown(u8),
}

impl From<messages::RequestMode> for RequestMode {
    fn from(r: messages::RequestMode) -> Self {
        match r {
            messages::RequestMode::FireAndForget => Self::FireAndForget,
            messages::RequestMode::ReportProgress => Self::ReportProgress,
        }
    }
}

impl TryFrom<RequestMode> for messages::RequestMode {
    type Error = DecodeRequestError;

    fn try_from(value: RequestMode) -> Result<Self, Self::Error> {
        match value {
            RequestMode::FireAndForget => Ok(Self::FireAndForget),
            RequestMode::ReportProgress => Ok(Self::ReportProgress),
            RequestMode::Unknown(other) => Err(DecodeRequestError::UnknownRequestMode(other)),
        }
    }
}

impl From<messages::Request> for Request {
    fn from(r: messages::Request) -> Self {
        let (payload, kind) = match r.payload {
            messages::RequestPayload::Announce { urn, rev } => (
                minicbor::to_vec(announce::Announce {
                    urn,
                    rev: rev.into(),
                })
                .unwrap(),
                RequestKind::Announce,
            ),
        };
        Request {
            headers: RequestHeaders {
                user_agent: r.user_agent.into(),
                kind,
                mode: r.mode.into(),
            },
            payload: Some(payload),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum DecodeRequestError {
    #[error("no payload for message kind which should have one")]
    MissingPayload,
    #[error(transparent)]
    BadPayloadEncoding(#[from] minicbor::decode::Error),
    #[error("unknown request kind: {0}")]
    UnknownRequestKind(u8),
    #[error("unknown request mode {0}")]
    UnknownRequestMode(u8),
}

impl TryFrom<Request> for messages::Request {
    type Error = DecodeRequestError;

    fn try_from(value: Request) -> Result<Self, Self::Error> {
        let payload_bytes = value.payload.ok_or(DecodeRequestError::MissingPayload)?;
        let payload = match value.headers.kind {
            RequestKind::Announce => {
                let announce::Announce { rev, urn } = minicbor::decode(&payload_bytes)?;
                messages::RequestPayload::Announce {
                    rev: rev.into(),
                    urn,
                }
            },
            RequestKind::Unknown(other) => {
                return Err(DecodeRequestError::UnknownRequestKind(other))
            },
        };
        Ok(messages::Request {
            mode: value.headers.mode.try_into()?,
            user_agent: value.headers.user_agent.into(),
            payload,
        })
    }
}

impl From<messages::Response> for Response {
    fn from(r: messages::Response) -> Self {
        let id = r.request_id.into();
        let (kind, payload) = match r.payload {
            messages::ResponsePayload::Ack => (ResponseKind::Ack, None),
            messages::ResponsePayload::Success => (ResponseKind::Success, None),
            messages::ResponsePayload::Progress(s) => (
                ResponseKind::Progress,
                Some(minicbor::to_vec(Progress(s)).unwrap()),
            ),
            messages::ResponsePayload::Error(s) => (
                ResponseKind::Error,
                Some(minicbor::to_vec(Error(s)).unwrap()),
            ),
        };
        Response {
            headers: ResponseHeaders {
                request_id: id,
                kind,
            },
            payload,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum DecodeResponseError {
    #[error(transparent)]
    BadPayloadEncoding(#[from] minicbor::decode::Error),
    #[error("missing payload")]
    MissingPayload,
    #[error("unknown response kind {0}")]
    UnknownResponseKind(u8),
}

impl TryFrom<Response> for messages::Response {
    type Error = DecodeResponseError;

    fn try_from(value: Response) -> Result<Self, Self::Error> {
        let id = value.headers.request_id.into();
        let payload = match value.headers.kind {
            ResponseKind::Ack => messages::ResponsePayload::Ack,
            ResponseKind::Error => {
                let payload_bytes = value.payload.ok_or(DecodeResponseError::MissingPayload)?;
                let Error(s) = minicbor::decode(&payload_bytes)?;
                messages::ResponsePayload::Error(s)
            },
            ResponseKind::Progress => {
                let payload_bytes = value.payload.ok_or(DecodeResponseError::MissingPayload)?;
                let Progress(s) = minicbor::decode(&payload_bytes)?;
                messages::ResponsePayload::Progress(s)
            },
            ResponseKind::Success => messages::ResponsePayload::Success,
            ResponseKind::Unknown(other) => {
                return Err(DecodeResponseError::UnknownResponseKind(other))
            },
        };
        Ok(messages::Response {
            request_id: id,
            payload,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RequestKind {
    Announce,
    Unknown(u8),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResponseKind {
    Ack,
    Success,
    Progress,
    Error,
    Unknown(u8),
}

#[derive(Clone, Debug, minicbor::Decode, minicbor::Encode, PartialEq)]
#[cbor(map)]
pub struct RequestHeaders {
    #[n(0)]
    pub(crate) user_agent: UserAgent,
    #[n(1)]
    pub(crate) kind: RequestKind,
    #[n(2)]
    pub(crate) mode: RequestMode,
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

#[derive(Clone, Debug, PartialEq, minicbor::Decode, minicbor::Encode)]
#[cbor(map)]
pub struct ResponseHeaders {
    #[n(0)]
    pub request_id: RequestId,
    #[n(1)]
    pub kind: ResponseKind,
}

impl minicbor::Encode for RequestKind {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        let val = match self {
            Self::Announce => 1,
            Self::Unknown(other) => *other,
        };
        e.u8(val)?;
        Ok(())
    }
}

impl<'b> minicbor::Decode<'b> for RequestKind {
    fn decode(d: &mut minicbor::Decoder<'b>) -> Result<Self, minicbor::decode::Error> {
        Ok(match d.u8()? {
            1 => Self::Announce,
            other => Self::Unknown(other),
        })
    }
}

impl minicbor::Encode for ResponseKind {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        let val = match self {
            Self::Ack => 1,
            Self::Success => 2,
            Self::Error => 3,
            Self::Progress => 4,
            Self::Unknown(other) => *other,
        };
        e.u8(val)?;
        Ok(())
    }
}

impl<'b> minicbor::Decode<'b> for ResponseKind {
    fn decode(d: &mut minicbor::Decoder<'b>) -> Result<Self, minicbor::decode::Error> {
        Ok(match d.u8()? {
            1 => Self::Ack,
            2 => Self::Success,
            3 => Self::Error,
            4 => Self::Progress,
            other => Self::Unknown(other),
        })
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

impl minicbor::Encode for RequestMode {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        let val = match self {
            Self::FireAndForget => 1,
            Self::ReportProgress => 2,
            Self::Unknown(other) => *other,
        };
        e.u8(val)?;
        Ok(())
    }
}

impl<'b> minicbor::Decode<'b> for RequestMode {
    fn decode(d: &mut minicbor::Decoder<'b>) -> Result<Self, minicbor::decode::Error> {
        Ok(match d.u8()? {
            1 => Self::FireAndForget,
            2 => Self::ReportProgress,
            other => Self::Unknown(other),
        })
    }
}

pub mod announce {
    use librad::git::Urn;
    use radicle_git_ext::Oid;

    #[derive(Clone, Debug, PartialEq, minicbor::Decode, minicbor::Encode)]
    pub struct Announce {
        #[n(0)]
        pub urn: Urn,
        #[n(1)]
        pub rev: Oid,
    }
}
