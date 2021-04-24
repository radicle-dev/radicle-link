// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::{
        btree_map::{self, Entry},
        BTreeMap,
        BTreeSet,
    },
    fmt::{Debug, Display},
    slice,
    vec,
};

use either::*;

use crate::keys::PublicKey;

use super::{generic, payload, sealed, Delegations, Direct};

pub mod error {
    use std::fmt::{Debug, Display};

    use thiserror::Error;

    use super::PublicKey;

    #[derive(Debug, Error)]
    #[non_exhaustive]
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

pub type IndirectlyDelegating<T, R, C> = generic::Identity<generic::Doc<T, Direct, R>, R, C>;

/// [`Delegations`] to either a [`PublicKey`]s directly, or another identity
/// (which itself must only contain [`Direct`] delegations).
#[derive(Clone, Debug)]
pub struct Indirect<T, R, C> {
    identities: Vec<IndirectlyDelegating<T, R, C>>,
    delegations: BTreeMap<PublicKey, Option<usize>>,
}

// `self.identities` is stored in insertion order, so we can't just derive
// `PartialEq`. It turns out that making `Identity` `Ord` (or `Hash`) is quite
// invasive, plus that we would need to store an extra unordered set. There is
// also not a hard guarantee the `Identity` invariants are maintained (namely
// the content hashes). So, let's keep this impl to tests, and assume `root` and
// `revision` identify the stored `IndirectlyDelegating`.
#[cfg(any(test, feature = "prop"))]
impl<T, R, C> PartialEq for Indirect<T, R, C>
where
    R: Ord,
    C: Ord,
{
    fn eq(&self, other: &Self) -> bool {
        self.delegations.len() == other.delegations.len()
            && self.identities.len() == other.identities.len()
            && self
                .delegations
                .keys()
                .zip(other.delegations.keys())
                .all(|(a, b)| a == b)
            && self
                .identities
                .iter()
                .map(|id| (&id.root, &id.revision))
                .collect::<BTreeSet<_>>()
                == other
                    .identities
                    .iter()
                    .map(|id| (&id.root, &id.revision))
                    .collect::<BTreeSet<_>>()
    }
}

impl<T, R, C> Indirect<T, R, C> {
    /// Build `Self` from an iterator of either [`PublicKey`]s or
    /// [`IndirectlyDelegating`] identities.
    ///
    /// # Errors
    ///
    /// * If a duplicate [`PublicKey`] is encountered (regardless of whether it
    ///   is a direct or indirect delegation ).
    /// * If a [`IndirectlyDelegating`] is encountered which refers to the same
    ///   root revision as a previous one.
    pub fn try_from_iter<I>(iter: I) -> Result<Self, error::FromIter<R>>
    where
        I: IntoIterator<Item = Either<PublicKey, IndirectlyDelegating<T, R, C>>>,
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
            Entry::Occupied(entry) => Err(DuplicateKey(*entry.key())),
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
                        insert(*key, Some(pos))?
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
    pub fn owner(&self, key: &PublicKey) -> Option<&IndirectlyDelegating<T, R, C>> {
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

    pub fn iter(&self) -> Iter<'_, T, R, C> {
        self.into_iter()
    }
}

/// Create an [`Indirect`] delegation from a single [`PublicKey`].
impl<T, R, C> From<PublicKey> for Indirect<T, R, C> {
    fn from(pk: PublicKey) -> Self {
        Self {
            identities: vec![],
            delegations: Some((pk, None)).into_iter().collect(),
        }
    }
}

/// Create an [`Indirect`] delegation from a single [`IndirectlyDelegating`].
impl<T, R, C> From<IndirectlyDelegating<T, R, C>> for Indirect<T, R, C> {
    fn from(id: IndirectlyDelegating<T, R, C>) -> Self {
        Self {
            identities: vec![id],
            delegations: Default::default(),
        }
    }
}

impl<T, R, C> From<Indirect<T, R, C>> for payload::ProjectDelegations<R>
where
    R: Clone + Ord,
{
    fn from(this: Indirect<T, R, C>) -> Self {
        this.into_iter()
            .map(|x| x.map_right(|id| id.urn()))
            .collect()
    }
}

/// Yields direct delegations as `Left(&PublicKey)`, and indirect ones as
/// `Right(&IndirectlyDelegating)`, with no duplicates. I.e. it holds that:
///
/// ```text
/// let x = Indirect::try_from_iter(y)?;
/// assert_eq!(Indirect::try_from_iter(x.iter().cloned())?, x)
/// ```
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct Iter<'a, T, R, C> {
    identities: slice::Iter<'a, IndirectlyDelegating<T, R, C>>,
    delegations: btree_map::Iter<'a, PublicKey, Option<usize>>,
}

impl<'a, T, R, C> Iter<'a, T, R, C> {
    /// Yield only the direct delegations.
    pub fn direct(self) -> impl Iterator<Item = &'a PublicKey> {
        self.filter_map(|d| d.either(Some, |_| None))
    }

    /// Yield only the indirect delegations.
    pub fn indirect(self) -> impl Iterator<Item = &'a IndirectlyDelegating<T, R, C>> {
        self.filter_map(|d| d.either(|_| None, Some))
    }
}

impl<'a, T, R, C> Iterator for Iter<'a, T, R, C> {
    type Item = Either<&'a PublicKey, &'a IndirectlyDelegating<T, R, C>>;

    fn next(&mut self) -> Option<Self::Item> {
        self.identities.next().map(Right).or_else(|| {
            while let Some((pk, pos)) = self.delegations.next() {
                if pos.is_none() {
                    return Some(Left(pk));
                }
            }

            None
        })
    }
}

impl<'a, T, R, C> IntoIterator for &'a Indirect<T, R, C> {
    type Item = Either<&'a PublicKey, &'a IndirectlyDelegating<T, R, C>>;
    type IntoIter = Iter<'a, T, R, C>;

    fn into_iter(self) -> Self::IntoIter {
        Iter {
            identities: self.identities.iter(),
            delegations: self.delegations.iter(),
        }
    }
}

/// Yields direct delegations as `Left(PublicKey)`, and indirect ones as
/// `Right(IndirectlyDelegating)`, with no duplicates. I.e. it holds that:
///
/// ```text
/// let x = Indirect::try_from_iter(y)?;
/// assert_eq!(Indirect::try_from_iter(x.into_iter())?, x)
/// ```
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct IntoIter<T, R, C> {
    identities: vec::IntoIter<IndirectlyDelegating<T, R, C>>,
    delegations: btree_map::IntoIter<PublicKey, Option<usize>>,
}

impl<T, R, C> IntoIter<T, R, C> {
    /// Yield only the direct delegations.
    pub fn direct(self) -> impl Iterator<Item = PublicKey> {
        self.filter_map(|d| d.either(Some, |_| None))
    }

    /// Yield only the indirect delegations.
    pub fn indirect(self) -> impl Iterator<Item = IndirectlyDelegating<T, R, C>> {
        self.filter_map(|d| d.either(|_| None, Some))
    }
}

impl<T, R, C> Iterator for IntoIter<T, R, C> {
    type Item = Either<PublicKey, IndirectlyDelegating<T, R, C>>;

    fn next(&mut self) -> Option<Self::Item> {
        self.identities.next().map(Right).or_else(|| {
            while let Some((pk, pos)) = self.delegations.next() {
                if pos.is_none() {
                    return Some(Left(pk));
                }
            }

            None
        })
    }
}

impl<T, R, C> IntoIterator for Indirect<T, R, C> {
    type Item = Either<PublicKey, IndirectlyDelegating<T, R, C>>;
    type IntoIter = IntoIter<T, R, C>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter {
            identities: self.identities.into_iter(),
            delegations: self.delegations.into_iter(),
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
