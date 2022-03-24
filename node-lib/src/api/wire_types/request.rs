// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::convert::{TryFrom, TryInto as _};

use super::{messages, Message};

pub(crate) type Request = Message<Headers>;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Mode {
    FireAndForget,
    ReportProgress,
    Unknown(u8),
}

impl From<messages::RequestMode> for Mode {
    fn from(r: messages::RequestMode) -> Self {
        match r {
            messages::RequestMode::FireAndForget => Self::FireAndForget,
            messages::RequestMode::ReportProgress => Self::ReportProgress,
        }
    }
}

impl TryFrom<Mode> for messages::RequestMode {
    type Error = DecodeError;

    fn try_from(value: Mode) -> Result<Self, Self::Error> {
        match value {
            Mode::FireAndForget => Ok(Self::FireAndForget),
            Mode::ReportProgress => Ok(Self::ReportProgress),
            Mode::Unknown(other) => Err(DecodeError::UnknownRequestMode(other)),
        }
    }
}

impl From<messages::Request> for Request {
    fn from(r: messages::Request) -> Self {
        let (payload, kind) = match r.payload {
            messages::RequestPayload::Announce(announce) => {
                (minicbor::to_vec(announce).unwrap(), Kind::Announce)
            },
            messages::RequestPayload::RequestPull(request_pull) => {
                (minicbor::to_vec(request_pull).unwrap(), Kind::RequestPull)
            },
        };
        Request {
            headers: Headers {
                user_agent: r.user_agent,
                kind,
                mode: r.mode.into(),
            },
            payload: Some(payload),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum DecodeError {
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
    type Error = DecodeError;

    fn try_from(value: Request) -> Result<Self, Self::Error> {
        let payload_bytes = value.payload.ok_or(DecodeError::MissingPayload)?;
        let payload = match value.headers.kind {
            Kind::Announce => messages::RequestPayload::Announce(minicbor::decode(&payload_bytes)?),
            Kind::RequestPull => {
                messages::RequestPayload::RequestPull(minicbor::decode(&payload_bytes)?)
            },
            Kind::Unknown(other) => return Err(DecodeError::UnknownRequestKind(other)),
        };
        Ok(messages::Request {
            mode: value.headers.mode.try_into()?,
            user_agent: value.headers.user_agent,
            payload,
        })
    }
}

// TODO: Introduce get-connected-peers, get-membership-info, and get-stats -- 2,
// 3, and 4 respectively.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    // CBOR encode and decode maps to 1
    Announce,
    // CBOR encode and decode maps to 5
    RequestPull,
    Unknown(u8),
}

#[derive(Clone, Debug, minicbor::Decode, minicbor::Encode, PartialEq)]
#[cbor(map)]
pub struct Headers {
    #[n(0)]
    pub(crate) user_agent: messages::UserAgent,
    #[n(1)]
    pub(crate) kind: Kind,
    #[n(2)]
    pub(crate) mode: Mode,
}

impl minicbor::Encode for Kind {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        let val = match self {
            Self::Announce => 1,
            Self::RequestPull => 5,
            Self::Unknown(other) => *other,
        };
        e.u8(val)?;
        Ok(())
    }
}

impl<'b> minicbor::Decode<'b> for Kind {
    fn decode(d: &mut minicbor::Decoder<'b>) -> Result<Self, minicbor::decode::Error> {
        Ok(match d.u8()? {
            1 => Self::Announce,
            5 => Self::RequestPull,
            other => Self::Unknown(other),
        })
    }
}

impl minicbor::Encode for Mode {
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

impl<'b> minicbor::Decode<'b> for Mode {
    fn decode(d: &mut minicbor::Decoder<'b>) -> Result<Self, minicbor::decode::Error> {
        Ok(match d.u8()? {
            1 => Self::FireAndForget,
            2 => Self::ReportProgress,
            other => Self::Unknown(other),
        })
    }
}
