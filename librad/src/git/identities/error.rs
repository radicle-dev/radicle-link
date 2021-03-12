// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use thiserror::Error;

use super::{
    super::{refs, storage, types::reference},
    local,
};
use crate::identities::{
    self,
    git::{Urn, VerificationError},
    urn,
};

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("the URN {0} does not exist")]
    NotFound(Urn),

    #[error("failed to build ref from URN")]
    RefFromUrn(#[from] reference::FromUrnError),

    #[error("failed to build URN from ref")]
    UrnFromRef(#[from] urn::FromRefLikeError),

    #[error("update of signed_refs failed")]
    Sigrefs(#[from] refs::stored::Error),

    #[error(transparent)]
    LocalId(#[from] local::ValidationError),

    #[error(transparent)]
    Verification(#[from] VerificationError),

    #[error(transparent)]
    Config(#[from] storage::config::Error),

    #[error(transparent)]
    Storage(#[from] storage::Error),

    #[error(transparent)]
    Verify(#[from] identities::git::error::Verify),

    #[error(transparent)]
    Merge(#[from] identities::git::error::Merge),

    #[error(transparent)]
    Load(#[from] identities::git::error::Load),

    #[error(transparent)]
    Store(#[from] identities::git::error::Store),

    #[error(transparent)]
    PersHist(#[from] identities::git::error::History<identities::git::PersonDoc>),

    #[error(transparent)]
    ProjHist(#[from] identities::git::error::History<identities::git::ProjectDoc>),

    #[error(transparent)]
    Git(#[from] git2::Error),
}
