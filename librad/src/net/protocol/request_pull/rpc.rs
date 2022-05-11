// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt;

use git_ref_format::RefString;
use minicbor::{Decode, Encode};

use crate::identities::git::Urn;

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub enum Response {
    #[n(0)]
    #[cbor(array)]
    Success(#[n(0)] Success),
    #[n(1)]
    #[cbor(array)]
    Error(#[n(0)] Error),
    #[n(2)]
    #[cbor(array)]
    Progress(#[n(0)] Progress),
}

impl From<Success> for Response {
    fn from(success: Success) -> Self {
        Self::Success(success)
    }
}

impl From<Error> for Response {
    fn from(error: Error) -> Self {
        Self::Error(error)
    }
}

impl From<Progress> for Response {
    fn from(progress: Progress) -> Self {
        Self::Progress(progress)
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, Encode, Decode)]
#[cbor(array)]
pub struct Request {
    #[n(0)]
    pub urn: Urn,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, Encode, Decode)]
#[cbor(array)]
pub struct Error {
    #[n(0)]
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Debug, Default, Hash, PartialEq, Eq, Encode, Decode)]
#[cbor(array)]
pub struct Success {
    #[n(0)]
    pub refs: Vec<Ref>,
    #[n(1)]
    pub pruned: Vec<RefString>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, Encode, Decode)]
#[cbor(array)]
pub struct Ref {
    #[n(0)]
    pub name: RefString,
    #[n(1)]
    pub oid: git_ext::Oid,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, Encode, Decode)]
#[cbor(array)]
pub struct Progress {
    #[n(0)]
    pub message: String,
}

impl fmt::Display for Progress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
