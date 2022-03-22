// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::fmt::{self, Debug, Display};

use git_ref_format::RefString;
use link_crypto::PeerId;
use link_git::protocol::ObjectId;
use thiserror::Error;

use crate::refs;

pub type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Layout {
    #[error("missing required refs: {0:?}")]
    MissingRequiredRefs(Vec<RefString>),

    #[error(transparent)]
    Other(#[from] Error),
}

impl Layout {
    pub fn other<E>(e: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::Other(Box::new(e))
    }
}

#[derive(Debug, Error)]
pub enum Prepare {
    #[error("identity verification failed")]
    Verification(#[source] Error),

    #[error("failed to look up ref {name}")]
    FindRef {
        name: RefString,
        #[source]
        source: Error,
    },

    #[error("failed scanning existing refs")]
    Scan {
        #[source]
        source: Error,
    },
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Validation {
    #[error("unexpected ref `{0}`")]
    Unexpected(RefString),

    #[error("malformed ref `{name}`")]
    Malformed {
        name: RefString,
        #[source]
        source: refs::parsed::Error,
    },

    #[error("missing expected ref {refname} of {remote}")]
    Missing {
        refname: RefString,
        remote: LocalOrRemote,
    },

    #[error("`refs/rad/id` is missing for {0}")]
    MissingRadId(LocalOrRemote),

    #[error("`refs/rad/signed_refs` is missing for {0}")]
    MissingSigRefs(LocalOrRemote),

    #[error("{name}: expected tip {expected}, but found {actual}")]
    MismatchedTips {
        expected: ObjectId,
        actual: ObjectId,
        name: RefString,
    },

    #[error("no data found for {0}")]
    NoData(LocalOrRemote),
}

#[derive(Clone, Copy, Debug)]
pub enum LocalOrRemote {
    LocalId,
    Remote(PeerId),
}

impl Display for LocalOrRemote {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::LocalId => f.write_str("the local peer id"),
            Self::Remote(id) => write!(f, "{}", id),
        }
    }
}

impl From<Option<PeerId>> for LocalOrRemote {
    fn from(opt: Option<PeerId>) -> Self {
        opt.map(Self::Remote).unwrap_or(Self::LocalId)
    }
}

impl From<PeerId> for LocalOrRemote {
    fn from(id: PeerId) -> Self {
        Self::Remote(id)
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum IdentityHistory<I: Debug + Send + Sync + 'static> {
    #[error("identities are of different types")]
    TypeMismatch { a: I, b: I },

    #[error(transparent)]
    Other(#[from] Error),
}

#[derive(Debug, Error)]
#[error("`rad/id` is behind and requires confirmation")]
pub struct ConfirmationRequired;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum OwnRad<T: Debug + Send + Sync + 'static> {
    #[error("error reading identity at `rad/id`")]
    Current(#[source] Error),

    #[error("`rad/id` is behind and requires confirmation")]
    ConfirmationRequired,

    #[error(transparent)]
    History(#[from] IdentityHistory<T>),

    #[error("failed to verify delegate identity {urn}")]
    Verify {
        urn: String,
        #[source]
        source: Error,
    },

    #[error("failed to track delegate {id}")]
    Track {
        id: PeerId,
        #[source]
        source: Error,
    },

    #[error("ref transaction failure")]
    Tx(#[source] Error),
}
