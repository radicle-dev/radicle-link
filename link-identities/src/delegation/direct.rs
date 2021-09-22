// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::collections::{btree_set, BTreeSet};

use thiserror::Error;

use crypto::PublicKey;
use data::{nonempty::Set as _, NonEmptyOrderedSet};

use super::Delegations;
use crate::{payload, sealed};

/// [`Delegations`] which delegate directly to a set of [`PublicKey`]s.
///
/// Untrusted input must be deserialised via [`payload::PersonDelegations`],
/// which ensures that duplicates in the source document translate to an error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Direct(NonEmptyOrderedSet<PublicKey>);

#[non_exhaustive]
#[derive(Debug, Error)]
pub enum Error {
    #[error("the set of keys provided was empty")]
    EmptyKeys,
}

impl Direct {
    pub fn new(key: PublicKey) -> Self {
        Self(NonEmptyOrderedSet::new(key))
    }

    pub fn insert(mut self, key: PublicKey) -> Self {
        self.0.insert(key);
        self
    }

    pub fn try_from_iter(keys: impl Iterator<Item = PublicKey>) -> Result<Self, Error> {
        let keys = keys.collect::<BTreeSet<_>>();
        match NonEmptyOrderedSet::from_maybe_empty(keys) {
            Some(keys) => Ok(Self(keys)),
            None => Err(Error::EmptyKeys),
        }
    }

    pub fn contains(&self, key: &PublicKey) -> bool {
        self.0.contains(key)
    }

    pub fn eligible(&self, votes: BTreeSet<&PublicKey>) -> BTreeSet<&PublicKey> {
        self.0.iter().filter(|pk| votes.contains(pk)).collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = &PublicKey> {
        self.into_iter()
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

impl From<payload::PersonDelegations> for Direct {
    fn from(payload: payload::PersonDelegations) -> Self {
        Self(payload.into())
    }
}

impl From<Direct> for BTreeSet<PublicKey> {
    fn from(here: Direct) -> Self {
        here.0.into_inner()
    }
}

impl From<Direct> for NonEmptyOrderedSet<PublicKey> {
    fn from(here: Direct) -> Self {
        here.0
    }
}

#[cfg(test)]
impl From<NonEmptyOrderedSet<PublicKey>> for Direct {
    fn from(set: NonEmptyOrderedSet<PublicKey>) -> Self {
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

impl Extend<PublicKey> for Direct {
    fn extend<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = PublicKey>,
    {
        self.0.extend(iter)
    }
}
