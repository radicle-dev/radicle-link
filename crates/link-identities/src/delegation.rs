// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::collections::BTreeSet;

use crypto::PublicKey;
use either::Either::{self, *};

use crate::{generic, sealed};

pub mod direct;
pub mod indirect;

pub use direct::Direct;
pub use indirect::Indirect;

/// Types which define trust delegations.
pub trait Delegations: sealed::Sealed {
    type Error;

    /// Given a set of votes (ie. signatures validated by the caller), return
    /// the subset which is valid for this delegation set.
    fn eligible(&self, votes: BTreeSet<&PublicKey>) -> Result<BTreeSet<&PublicKey>, Self::Error>;

    /// The threshold of [`Delegations::eligible`] votes required to form a
    /// quorum.
    ///
    /// Nb.: "threshold" means that there must be `quorum_threshold() + 1` votes
    /// to form a quorum.
    fn quorum_threshold(&self) -> usize;
}

//// Forwarding impls for `Doc` and `Identity`

impl<T, D, R> Delegations for generic::Doc<T, D, R>
where
    D: Delegations,
{
    type Error = D::Error;

    fn eligible(&self, votes: BTreeSet<&PublicKey>) -> Result<BTreeSet<&PublicKey>, Self::Error> {
        self.delegations.eligible(votes)
    }

    fn quorum_threshold(&self) -> usize {
        self.delegations.quorum_threshold()
    }
}

impl<T, R, C> Delegations for generic::Identity<T, R, C>
where
    T: Delegations,
{
    type Error = T::Error;

    fn eligible(&self, votes: BTreeSet<&PublicKey>) -> Result<BTreeSet<&PublicKey>, Self::Error> {
        self.doc.eligible(votes)
    }

    fn quorum_threshold(&self) -> usize {
        self.doc.quorum_threshold()
    }
}

/// "Existentialised" delegations.
#[derive(Clone, Debug)]
pub enum SomeDelegations<T, R: Ord, C: Ord> {
    Direct(Direct),
    Indirect(Indirect<T, R, C>),
}

impl<T, R: Ord, C: Ord> Delegations for SomeDelegations<T, R, C> {
    type Error = Either<<Direct as Delegations>::Error, <Indirect<T, R, C> as Delegations>::Error>;

    fn eligible(&self, votes: BTreeSet<&PublicKey>) -> Result<BTreeSet<&PublicKey>, Self::Error> {
        match self {
            SomeDelegations::Direct(direct) => Ok(direct.eligible(votes)),
            SomeDelegations::Indirect(indirect) => indirect.eligible(votes).map_err(Right),
        }
    }

    fn quorum_threshold(&self) -> usize {
        match self {
            SomeDelegations::Direct(direct) => direct.quorum_threshold(),
            SomeDelegations::Indirect(indirect) => indirect.quorum_threshold(),
        }
    }
}

impl<T, R: Ord, C: Ord> sealed::Sealed for SomeDelegations<T, R, C> {}

impl<T, R: Ord, C: Ord> PartialEq for SomeDelegations<T, R, C> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Direct(this), Self::Direct(that)) => this == that,
            (Self::Indirect(this), Self::Indirect(that)) => indirect::test::eq(this, that),
            (_, _) => false,
        }
    }
}
