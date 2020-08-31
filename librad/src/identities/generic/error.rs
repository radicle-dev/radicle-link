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
pub enum Verify<Revision, ContentId, Delegation, Iter>
where
    Revision: Display + Debug + 'static,
    ContentId: Display + Debug + 'static,
    Delegation: std::error::Error + 'static,
    Iter: std::error::Error + 'static,
{
    #[error("no valid signatures over {0} in {1}")]
    NoValidSignatures(Revision, ContentId),

    #[error("quorum not reached")]
    Quorum,

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
    Delegation(#[source] Delegation),

    #[error("error traversing the identity history")]
    Iter(#[source] Iter),
}
