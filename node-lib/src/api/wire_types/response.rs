// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::convert::TryFrom;

use super::{messages, Error, Message, Progress};

pub(crate) type Response = Message<Headers>;

impl Response {
    fn into_message(
        &self,
        awaiting: super::request::Kind,
    ) -> Result<messages::Response, DecodeError> {
        let id = self.headers.request_id;
        let payload = match self.headers.kind {
            Kind::Ack => messages::ResponsePayload::Ack,
            Kind::Error => {
                let payload_bytes = self.payload.ok_or(DecodeError::MissingPayload)?;
                let Error(s) = minicbor::decode(&payload_bytes)?;
                messages::ResponsePayload::Error(s)
            },
            Kind::Progress => {
                let payload_bytes = self.payload.ok_or(DecodeError::MissingPayload)?;
                let Progress(s) = minicbor::decode(&payload_bytes)?;
                messages::ResponsePayload::Progress(s)
            },
            Kind::Success => {
                use super::request::Kind::*;
                let payload = match awaiting {
                    RequestPull => {
                        let payload_bytes = self.payload.ok_or(DecodeError::MissingPayload)?;
                        Some(messages::SuccessPayload::RequestPull(minicbor::decode(
                            &payload_bytes,
                        )?))
                    },
                    _ => None,
                };
                messages::ResponsePayload::Success(payload)
            },
            Kind::Unknown(other) => return Err(DecodeError::UnknownResponseKind(other)),
        };
        Ok(messages::Response {
            request_id: id,
            payload,
        })
    }
}

impl From<messages::Response> for Response {
    fn from(r: messages::Response) -> Self {
        let id = r.request_id;
        let (kind, payload) = match r.payload {
            messages::ResponsePayload::Ack => (Kind::Ack, None),
            messages::ResponsePayload::Success(payload) => {
                (Kind::Success, payload.map(|p| minicbor::to_vec(p).unwrap()))
            },
            messages::ResponsePayload::Progress(s) => {
                (Kind::Progress, Some(minicbor::to_vec(Progress(s)).unwrap()))
            },
            messages::ResponsePayload::Error(s) => {
                (Kind::Error, Some(minicbor::to_vec(Error(s)).unwrap()))
            },
        };
        Response {
            headers: Headers {
                request_id: id,
                kind,
            },
            payload,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum DecodeError {
    #[error(transparent)]
    BadPayloadEncoding(#[from] minicbor::decode::Error),
    #[error("missing payload")]
    MissingPayload,
    #[error("unknown response kind {0}")]
    UnknownResponseKind(u8),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Ack,
    Success,
    Progress,
    Error,
    Unknown(u8),
}

#[derive(Clone, Debug, PartialEq, minicbor::Decode, minicbor::Encode)]
#[cbor(map)]
pub struct Headers {
    #[n(0)]
    pub request_id: messages::RequestId,
    #[n(1)]
    pub kind: Kind,
}

impl minicbor::Encode for Kind {
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

impl<'b> minicbor::Decode<'b> for Kind {
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
