// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::fmt::Debug;

use bstr::BString;
use link_crypto::PeerId;
use link_git::protocol::ObjectId;
use thiserror::Error;

pub type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Layout {
    #[error("missing required refs: {0:?}")]
    MissingRequiredRefs(Vec<BString>),

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
pub enum Prepare<V, R>
where
    V: std::error::Error + Send + Sync + 'static,
    R: std::error::Error + Send + Sync + 'static,
{
    #[error("identify verification failed")]
    Verification(#[source] V),

    #[error("failed to look up ref {name}")]
    FindRef {
        name: BString,
        #[source]
        source: R,
    },
}

#[derive(Debug, Error, Eq, PartialEq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Validation {
    #[error("unrecognised format: {0}")]
    Unrecognised(BString),

    #[error("unexpected: {0}")]
    Unexpected(BString),

    #[error("missing expected ref {refname} of {remote}")]
    Missing { refname: BString, remote: PeerId },

    #[error("`refs/rad/id` is missing for {0}")]
    MissingRadId(PeerId),

    #[error("`refs/rad/signed_refs` is missing for {0}")]
    MissingSigRefs(PeerId),

    #[error("{name}: signed tip at {signed}, but actual is {actual}")]
    MismatchedTips {
        signed: ObjectId,
        actual: ObjectId,
        name: BString,
    },

    #[error("strange refname or category: {0}")]
    Strange(BString),

    #[error("strange refname or prunable ref: {0}")]
    StrangeOrPrunable(BString),

    #[error("tracking {0}, but no data was pulled yet")]
    NoData(PeerId),
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
