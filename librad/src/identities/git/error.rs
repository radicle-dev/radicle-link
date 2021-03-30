// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{fmt::Debug, path::PathBuf};

use thiserror::Error;

use crate::{
    identities::{
        delegation::indirect::error::FromIter as DelegationsFromIterError,
        generic,
        sign,
        ContentId,
        Revision,
    },
    internal::canonical::CjsonError,
};

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Load {
    #[error("the identity document could not be resolved")]
    MissingDoc,

    #[error("the root revision of the identity document could not be resolved")]
    MissingRoot,

    #[error(
        "document hash does not match stored hash. \
        Perhaps the document is not in canonical form?"
    )]
    DigestMismatch,

    #[error("expected blob at path `{0:?}`, got {1:?}")]
    NotABlob(PathBuf, Option<git2::ObjectType>),

    #[error(transparent)]
    Delegation(#[from] DelegationsFromIterError<Revision>),

    #[error(transparent)]
    Signatures(#[from] self::Signatures),

    #[error(transparent)]
    Cjson(#[from] CjsonError),

    #[error(transparent)]
    Revision(#[from] multihash::DecodeOwnedError),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Git(#[from] git2::Error),
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Store {
    #[error(transparent)]
    Load(#[from] self::Load),

    #[error("failed to produce a signature")]
    Signer(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),

    #[error(transparent)]
    Cjson(#[from] CjsonError),

    #[error(transparent)]
    Git(#[from] git2::Error),
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Merge {
    #[error("attempt to update an identity not previously signed by us")]
    ForeignBase,

    #[error("merge candidates must have the same root")]
    RootMismatch,

    #[error(
        "merge candidates must either have the same revision, \
        or the RHS must be a direct successor of the LHS"
    )]
    RevisionMismatch,

    #[error("failed to produce a signature")]
    Signer(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),

    #[error(transparent)]
    Git(#[from] git2::Error),
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Signatures {
    #[error("Invalid utf8")]
    Utf8,

    #[error(transparent)]
    Signatures(#[from] sign::error::Signatures),

    #[error(transparent)]
    Git(#[from] git2::Error),
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum VerifyProject {
    #[error("error resolving latest head")]
    Lookup(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),

    #[error(transparent)]
    Verification(#[from] generic::error::Verify<Revision, ContentId>),

    #[error(transparent)]
    VerifyPerson(#[from] self::VerifyPerson),

    #[error(transparent)]
    Delegation(#[from] DelegationsFromIterError<Revision>),

    #[error(transparent)]
    Load(#[from] self::Load),

    #[error(transparent)]
    Git(#[from] git2::Error),
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum VerifyPerson {
    #[error("Revision {revision} of {root} not in ancestry path of {head}")]
    NotInAncestryPath {
        revision: Revision,
        root: Revision,
        head: ContentId,
    },

    #[error(transparent)]
    Verification(#[from] generic::error::Verify<Revision, ContentId>),

    #[error(transparent)]
    Git(#[from] git2::Error),
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Verify {
    #[error(transparent)]
    Project(#[from] VerifyProject),

    #[error(transparent)]
    Person(#[from] VerifyPerson),
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum History<T: Debug> {
    #[error("unrelated histories")]
    Fork {
        left: super::VerifiedIdentity<T>,
        right: super::VerifiedIdentity<T>,
    },

    #[error(transparent)]
    Git(#[from] git2::Error),
}
