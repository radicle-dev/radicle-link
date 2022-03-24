// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use rand::Rng;

use super::{announce, request_pull};

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, minicbor::Decode, minicbor::Encode,
)]
#[cbor(transparent)]
pub struct RequestId(#[n(0)] minicbor::bytes::ByteVec);

impl From<RequestId> for Vec<u8> {
    fn from(r: RequestId) -> Self {
        r.0.to_vec()
    }
}

impl From<Vec<u8>> for RequestId {
    fn from(raw: Vec<u8>) -> Self {
        Self(raw.into())
    }
}

impl AsRef<[u8]> for RequestId {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Default for RequestId {
    fn default() -> Self {
        let mut rng = rand::thread_rng();
        let bytes: [u8; 16] = rng.gen();
        RequestId(bytes.to_vec().into())
    }
}

#[derive(Clone, Debug, minicbor::Decode, minicbor::Encode, PartialEq)]
#[cbor(transparent)]
pub struct UserAgent(#[n(0)] String);

impl From<String> for UserAgent {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for UserAgent {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl<'a> From<&'a UserAgent> for &'a str {
    fn from(ua: &'a UserAgent) -> &'a str {
        &ua.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RequestMode {
    FireAndForget,
    ReportProgress,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub user_agent: UserAgent,
    pub mode: RequestMode,
    pub payload: RequestPayload,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RequestPayload {
    Announce(announce::Request),
    RequestPull(request_pull::Request),
}

impl From<announce::Request> for RequestPayload {
    fn from(x: announce::Request) -> Self {
        Self::Announce(x)
    }
}

impl From<request_pull::Request> for RequestPayload {
    fn from(x: request_pull::Request) -> Self {
        Self::RequestPull(x)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Response<P> {
    pub request_id: RequestId,
    pub payload: ResponsePayload<P>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ResponsePayload<P> {
    Ack,
    Progress(String),
    Error(String),
    Success(P),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SomeSuccess {
    Announce(announce::Response),
    RequestPull(request_pull::Response),
}

impl From<announce::Response> for SomeSuccess {
    fn from(x: announce::Response) -> Self {
        Self::Announce(x)
    }
}

impl From<request_pull::Response> for SomeSuccess {
    fn from(x: request_pull::Response) -> Self {
        Self::RequestPull(x)
    }
}

impl minicbor::Encode for SomeSuccess {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        match self {
            SomeSuccess::Announce(x) => e.encode(x)?.ok(),
            SomeSuccess::RequestPull(x) => e.encode(x)?.ok(),
        }
    }
}

pub trait RecvPayload: for<'b> minicbor::Decode<'b> + Send + Sync + 'static {}
impl<T> RecvPayload for T where for<'b> T: minicbor::Decode<'b> + Send + Sync + 'static {}

pub trait SendPayload: minicbor::Encode + Send + Sync + 'static {}
impl<T> SendPayload for T where T: minicbor::Encode + Send + Sync + 'static {}
