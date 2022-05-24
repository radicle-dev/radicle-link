// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    ops::Deref,
};

use git_ref_format::RefString;
use itertools::Itertools as _;
use link_crypto::PeerId;
use link_git::protocol::{oid, ObjectId};

pub mod error {
    use link_crypto::PeerId;
    use thiserror::Error;

    #[derive(Debug, Error)]
    #[non_exhaustive]
    pub enum Combine<E: std::error::Error + 'static> {
        #[error("required sigrefs of {0} not found")]
        NotFound(PeerId),

        #[error(transparent)]
        Load(#[from] E),
    }
}

pub trait SignedRefs {
    type Oid: AsRef<oid> + Into<ObjectId> + 'static;
    type Error: std::error::Error + Send + Sync + 'static;

    /// Load the signed refs `of` remote peer, limiting the tracking graph depth
    /// to `cutoff`.
    ///
    /// The URN context is implied. `None` means the sigrefs could not be found.
    fn load(&self, of: &PeerId, cutoff: usize) -> Result<Option<Sigrefs<Self::Oid>>, Self::Error>;

    fn load_at(
        &self,
        treeish: impl Into<ObjectId>,
        of: &PeerId,
        cutoff: usize,
    ) -> Result<Option<Sigrefs<Self::Oid>>, Self::Error>;

    /// Compute and update the sigrefs for the local peer.
    ///
    /// A `None` return value denotes a no-op (ie. the sigrefs were already
    /// up-to-date).
    fn update(&self) -> Result<Option<Self::Oid>, Self::Error>;
}

#[derive(Debug)]
pub struct Sigrefs<Oid> {
    pub at: Oid,
    pub refs: HashMap<RefString, Oid>,
    pub remotes: BTreeSet<PeerId>,
}

#[derive(Debug)]
pub struct Flattened<Oid> {
    /// Signed refs per tracked peer
    pub refs: BTreeMap<PeerId, Refs<Oid>>,
    /// Flattened remotes, with cutoff as per replication factor.
    pub remotes: BTreeSet<PeerId>,
}

impl<T> Default for Flattened<T> {
    fn default() -> Self {
        Self {
            refs: BTreeMap::new(),
            remotes: BTreeSet::new(),
        }
    }
}

#[derive(Debug)]
pub struct Combined<Oid>(BTreeMap<PeerId, Sigrefs<Oid>>);

impl<Oid> Combined<Oid> {
    pub fn flattened(self) -> Flattened<Oid> {
        let mut refs = BTreeMap::new();
        let mut remotes = BTreeSet::new();
        for (id, sigrefs) in self.0 {
            refs.insert(
                id,
                Refs {
                    at: sigrefs.at,
                    refs: sigrefs.refs,
                },
            );
            remotes.extend(sigrefs.remotes);
        }

        Flattened { refs, remotes }
    }
}

impl<Oid> Deref for Combined<Oid> {
    type Target = BTreeMap<PeerId, Sigrefs<Oid>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<Oid> Default for Combined<Oid> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<Oid> From<Combined<Oid>> for Flattened<Oid> {
    fn from(a: Combined<Oid>) -> Self {
        a.flattened()
    }
}

impl<'a, Oid> IntoIterator for &'a Combined<Oid> {
    type Item = <&'a BTreeMap<PeerId, Sigrefs<Oid>> as IntoIterator>::Item;
    type IntoIter = <&'a BTreeMap<PeerId, Sigrefs<Oid>> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

#[derive(Debug)]
pub struct Refs<Oid> {
    /// Head of the `rad/signed_refs` the refs were loaded from.
    pub at: Oid,
    /// The signed `(refname, head)` pairs.
    pub refs: HashMap<RefString, Oid>,
}

impl<'a, Oid> IntoIterator for &'a Refs<Oid> {
    type Item = <&'a HashMap<RefString, Oid> as IntoIterator>::Item;
    type IntoIter = <&'a HashMap<RefString, Oid> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.refs.iter()
    }
}

pub struct Select<'a> {
    pub must: &'a BTreeSet<PeerId>,
    pub may: &'a BTreeSet<PeerId>,
    pub cutoff: usize,
}

pub fn combined<S>(
    s: &S,
    Select { must, may, cutoff }: Select,
) -> Result<Combined<S::Oid>, error::Combine<S::Error>>
where
    S: SignedRefs,
{
    let must = must.iter().map(|id| {
        SignedRefs::load(s, id, cutoff)
            .map_err(error::Combine::from)
            .and_then(|sr| match sr {
                None => Err(error::Combine::NotFound(*id)),
                Some(sr) => Ok((id, sr)),
            })
    });
    let may = may
        .iter()
        .filter_map(|id| match SignedRefs::load(s, id, cutoff) {
            Ok(None) => None,
            Ok(Some(sr)) => Some(Ok((id, sr))),
            Err(e) => Some(Err(e.into())),
        });

    must.chain(may)
        .fold_ok(Combined::default(), |mut acc, (id, sigrefs)| {
            acc.0.insert(*id, sigrefs);
            acc
        })
}
