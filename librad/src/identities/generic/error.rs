// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

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
