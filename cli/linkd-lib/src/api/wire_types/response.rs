// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::convert::TryFrom;

use super::{messages, Error, Message, Progress};

pub type Response = Message<Headers>;

impl<P> TryFrom<Response> for messages::Response<P>
where
    P: messages::RecvPayload,
{
    type Error = DecodeError;

    fn try_from(resp: Response) -> Result<Self, Self::Error> {
        let payload = match resp.headers.kind {
            Kind::Ack => messages::ResponsePayload::Ack,
            Kind::Error => {
                let payload_bytes = resp.payload.as_ref().ok_or(DecodeError::MissingPayload)?;
                let Error(s) = minicbor::decode(payload_bytes)?;
                messages::ResponsePayload::Error(s)
            },
            Kind::Progress => {
                let payload_bytes = resp.payload.as_ref().ok_or(DecodeError::MissingPayload)?;
                let Progress(s) = minicbor::decode(payload_bytes)?;
                messages::ResponsePayload::Progress(s)
            },
            Kind::Success => {
                let payload_bytes = resp.payload.as_ref().ok_or(DecodeError::MissingPayload)?;
                let payload = minicbor::decode::<P>(payload_bytes)?;
                messages::ResponsePayload::Success(payload)
            },
            Kind::Unknown(other) => return Err(DecodeError::UnknownResponseKind(other)),
        };
        Ok(messages::Response {
            request_id: resp.headers.request_id,
            payload,
        })
    }
}

impl<P> From<messages::Response<P>> for Response
where
    P: minicbor::Encode,
{
    fn from(r: messages::Response<P>) -> Self {
        let id = r.request_id;
        let (kind, payload) = match r.payload {
            messages::ResponsePayload::Ack => (Kind::Ack, None),
            messages::ResponsePayload::Success(payload) => {
                (Kind::Success, Some(minicbor::to_vec(payload).unwrap()))
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
pub enum DecodeError {
    #[error(transparent)]
    BadPayloadEncoding(#[from] minicbor::decode::Error),
    #[error("missing payload")]
    MissingPayload,
    #[error("unknown response kind {0}")]
    UnknownResponseKind(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Ack,
    Success,
    Progress,
    Error,
    Unknown(u8),
}

#[derive(Clone, Debug, PartialEq, Eq, minicbor::Decode, minicbor::Encode)]
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
