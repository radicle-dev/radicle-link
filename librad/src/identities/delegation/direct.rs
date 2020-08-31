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

use std::collections::{btree_set, BTreeSet};

use crate::keys::PublicKey;

use super::{payload, sealed, Delegations};

/// [`Delegations`] which delegate directly to a set of [`PublicKey`]s.
///
/// The only way to construct a [`Direct`] value is `From`
/// [`payload::UserDelegations`], which ensures that duplicates in the source
/// document translate to an error.
#[derive(Clone, Debug, PartialEq)]
pub struct Direct(BTreeSet<PublicKey>);

impl Direct {
    pub fn eligible(&self, votes: BTreeSet<&PublicKey>) -> BTreeSet<&PublicKey> {
        self.0.iter().filter(|pk| votes.contains(pk)).collect()
    }
}

impl Delegations for Direct {
    type Error = !;

    fn eligible(&self, votes: BTreeSet<&PublicKey>) -> Result<BTreeSet<&PublicKey>, Self::Error> {
        Ok(self.eligible(votes))
    }

    fn quorum_threshold(&self) -> usize {
        self.0.len() / 2
    }
}

impl sealed::Sealed for Direct {}

impl From<payload::UserDelegations> for Direct {
    fn from(payload: payload::UserDelegations) -> Self {
        Self(payload.into())
    }
}

#[cfg(test)]
impl From<BTreeSet<PublicKey>> for Direct {
    fn from(set: BTreeSet<PublicKey>) -> Self {
        Self(set)
    }
}

impl<'a> IntoIterator for &'a Direct {
    type Item = &'a PublicKey;
    type IntoIter = btree_set::Iter<'a, PublicKey>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl IntoIterator for Direct {
    type Item = PublicKey;
    type IntoIter = btree_set::IntoIter<PublicKey>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}
