// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use rand::Rng;

use librad::git::Urn;

#[derive(Clone, Debug, PartialEq)]
pub struct RequestId(Vec<u8>);

#[derive(Clone, Debug, PartialEq)]
pub struct UserAgent(String);

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
    Announce { urn: Urn, rev: git2::Oid },
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
    Success,
}

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

impl From<String> for UserAgent {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl<'a> From<&'a UserAgent> for &'a str {
    fn from(ua: &'a UserAgent) -> &'a str {
        &ua.0
    }
}

impl Default for RequestId {
    fn default() -> Self {
        let mut rng = rand::thread_rng();
        let bytes: [u8; 16] = rng.gen();
        RequestId(bytes.to_vec())
    }
}
