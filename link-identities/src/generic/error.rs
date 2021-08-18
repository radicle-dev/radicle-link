// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::fmt::{Debug, Display};

use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Verify<Revision, ContentId>
where
    Revision: Display + Debug + 'static,
    ContentId: Display + Debug + 'static,
{
    #[error("one or more invalid signatures")]
    SignatureVerification,

    #[error("empty signatures")]
    NoSignatures,

    #[error("quorum not reached")]
    Quorum,

    #[error("quorum on parent not reached")]
    ParentQuorum,

    #[error("expected parent {expected}, found {actual}")]
    ParentMismatch {
        expected: Revision,
        actual: Revision,
    },

    #[error("unexpected parent of {0}: {1}")]
    DanglingParent(ContentId, ContentId),

    #[error("parent revision `{0}` missing")]
    MissingParent(Revision),

    #[error("identities do not refer to the same root")]
    RootMismatch {
        expected: Revision,
        actual: Revision,
    },

    #[error("empty history")]
    EmptyHistory,

    #[error("non-eligible delegation")]
    Eligibility(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),

    #[error("error traversing the identity history")]
    History(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),
}

impl<R, C> Verify<R, C>
where
    R: Display + Debug + 'static,
    C: Display + Debug + 'static,
{
    pub fn eligibility<E>(e: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::Eligibility(Box::new(e))
    }

    pub fn history<E>(e: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::History(Box::new(e))
    }
}
