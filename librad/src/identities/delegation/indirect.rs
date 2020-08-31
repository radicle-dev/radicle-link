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

use std::{
    collections::{
        btree_map::{self, Entry},
        BTreeMap,
        BTreeSet,
    },
    fmt::{Debug, Display},
};

use either::Either;

use crate::keys::PublicKey;

use super::{generic, sealed, Delegations, Direct};

pub mod error {
    use std::fmt::{Debug, Display};

    use thiserror::Error;

    use super::PublicKey;

    #[derive(Debug, Error)]
    pub enum FromIter<R: Display + Debug> {
        #[error("duplicate key `{0}`")]
        DuplicateKey(PublicKey),

        #[error("duplicate identity with root `{0}`")]
        DuplicateIdentity(R),
    }

    #[derive(Debug, Error, Eq, PartialEq)]
    #[error("double vote")]
    pub struct DoubleVote;
}

pub type DirectlyDelegating<T, R, C> = generic::Identity<generic::Doc<T, Direct, R>, R, C>;

/// [`Delegations`] to either a [`PublicKey`]s directly, or another identity
/// (which itself must only contain [`Direct`] delegations).
#[derive(Clone, Debug, PartialEq)]
pub struct Indirect<T, R, C> {
    identities: Vec<DirectlyDelegating<T, R, C>>,
    delegations: BTreeMap<PublicKey, Option<usize>>,
}

impl<T, R, C> Indirect<T, R, C> {
    /// Build `Self` from an iterator of either [`PublicKey`]s or
    /// [`DirectlyDelegating`] identities.
    ///
    /// # Errors
    ///
    /// * If a duplicate [`PublicKey`] is encountered (regardless of whether it
    ///   is a direct or indirect delegation ).
    /// * If a [`DirectlyDelegating`] is encountered which refers to the same
    ///   root revision as a previous one.
    pub fn try_from_iter<I>(iter: I) -> Result<Self, error::FromIter<R>>
    where
        I: IntoIterator<Item = Either<PublicKey, DirectlyDelegating<T, R, C>>>,
        R: Clone + Display + Debug + Ord,
    {
        use error::FromIter::*;

        let mut ids = Vec::new();
        let mut dels = BTreeMap::new();
        let mut roots = BTreeSet::new();

        let mut insert = |key: PublicKey, pos: Option<usize>| match dels.entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(pos);
                Ok(())
            },
            Entry::Occupied(entry) => Err(DuplicateKey(entry.key().clone())),
        };

        for d in iter {
            match d {
                Either::Left(key) => insert(key, None)?,
                Either::Right(id) => {
                    if !roots.insert(id.root.clone()) {
                        return Err(DuplicateIdentity(id.root));
                    }

                    ids.push(id);
                    let pos = ids.len() - 1;

                    for key in &ids[pos].doc.delegations {
                        insert(key.clone(), Some(pos))?
                    }
                },
            }
        }

        Ok(Self {
            identities: ids,
            delegations: dels,
        })
    }

    /// Get the owning [`generic::Identity`] of the given key, if any.
    pub fn owner(&self, key: &PublicKey) -> Option<&DirectlyDelegating<T, R, C>> {
        self.delegations
            .get(key)
            .and_then(|idx| idx.map(|idx| &self.identities[idx]))
    }

    /// In addition to checking whether the given [`PublicKey`]s are in the set
    /// of delegations, this also ensures that no two keys owned by the same
    /// indirect delegations are being used.
    ///
    /// If this is found to be the case, a [`error::DoubleVote`] error is
    /// returned.
    pub fn eligible(
        &self,
        votes: BTreeSet<&PublicKey>,
    ) -> Result<BTreeSet<&PublicKey>, error::DoubleVote> {
        let mut id_votes = BTreeSet::new();
        self.delegations
            .iter()
            .filter(|(k, _)| votes.contains(k))
            .try_fold(BTreeSet::new(), |mut acc, (k, idx)| {
                if let Some(id) = idx {
                    if !id_votes.insert(id) {
                        return Err(error::DoubleVote);
                    }
                }

                acc.insert(k);
                Ok(acc)
            })
    }
}

#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct Iter<'a, T, R, C> {
    inner: btree_map::Iter<'a, PublicKey, Option<usize>>,
    identities: &'a [DirectlyDelegating<T, R, C>],
}

impl<'a, T, R, C> Iterator for Iter<'a, T, R, C> {
    type Item = Either<&'a PublicKey, &'a DirectlyDelegating<T, R, C>>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(key, pos)| match pos {
            None => Either::Left(key),
            Some(pos) => Either::Right(&self.identities[*pos]),
        })
    }
}

impl<'a, T, R, C> IntoIterator for &'a Indirect<T, R, C> {
    type Item = Either<&'a PublicKey, &'a DirectlyDelegating<T, R, C>>;
    type IntoIter = Iter<'a, T, R, C>;

    fn into_iter(self) -> Self::IntoIter {
        Iter {
            inner: self.delegations.iter(),
            identities: &self.identities,
        }
    }
}

#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct IntoIter<T, R, C> {
    inner: btree_map::IntoIter<PublicKey, Option<usize>>,
    identities: Vec<DirectlyDelegating<T, R, C>>,
}

impl<T: Clone, R: Clone, C: Clone> Iterator for IntoIter<T, R, C> {
    type Item = Either<PublicKey, DirectlyDelegating<T, R, C>>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(key, pos)| match pos {
            None => Either::Left(key),
            Some(pos) => Either::Right(self.identities[pos].clone()),
        })
    }
}

impl<T: Clone, R: Clone, C: Clone> IntoIterator for Indirect<T, R, C> {
    type Item = Either<PublicKey, DirectlyDelegating<T, R, C>>;
    type IntoIter = IntoIter<T, R, C>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter {
            inner: self.delegations.into_iter(),
            identities: self.identities,
        }
    }
}

impl<T, R, C> Delegations for Indirect<T, R, C> {
    type Error = error::DoubleVote;

    fn eligible(&self, votes: BTreeSet<&PublicKey>) -> Result<BTreeSet<&PublicKey>, Self::Error> {
        self.eligible(votes)
    }

    fn quorum_threshold(&self) -> usize {
        let direct = self
            .delegations
            .iter()
            .filter(|(_, idx)| idx.is_none())
            .count();
        let indirect = self.identities.len();

        (direct + indirect) / 2
    }
}

impl<T, R, C> sealed::Sealed for Indirect<T, R, C> {}
