// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::net::protocol::request_pull;
use rand::Rng;

use super::{announce::Announce, request_pull::RequestPull, wire_types};

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, minicbor::Decode, minicbor::Encode,
)]
pub struct RequestId(#[cbor(n(0), with = "minicbor::bytes")] Vec<u8>);

impl From<RequestId> for Vec<u8> {
    fn from(r: RequestId) -> Self {
        r.0
    }
}

impl From<Vec<u8>> for RequestId {
    fn from(raw: Vec<u8>) -> Self {
        Self(raw)
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
        RequestId(bytes.to_vec())
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
    Announce(Announce),
    RequestPull(RequestPull),
}

impl From<Announce> for RequestPayload {
    fn from(a: Announce) -> Self {
        Self::Announce(a)
    }
}

impl From<RequestPull> for RequestPayload {
    fn from(rp: RequestPull) -> Self {
        Self::RequestPull(rp)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Response {
    pub request_id: RequestId,
    pub payload: ResponsePayload,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ResponsePayload {
    Ack,
    Progress(String),
    Error(String),
    Success(Option<SuccessPayload>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SuccessPayload {
    RequestPull(request_pull::Success),
}

impl minicbor::Encode for SuccessPayload {
    fn encode<W: minicbor::encode::Write>(&self, e: &mut minicbor::Encoder<W>) -> Result<(), minicbor::encode::Error<W::Error>> {
        match self {
            Self::RequestPull(p) => p.encode(e),
        }
    }
}
