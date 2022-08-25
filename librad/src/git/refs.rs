// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::{btree_map, BTreeMap},
    fmt::{self, Debug},
    iter::FromIterator,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    path::Path,
};

mod serde_impls;

use git_ext::{is_not_found_err, reference};
use link_canonical::{Cjson, CjsonError};
use serde::{
    de,
    ser::{self, SerializeStruct},
    Deserialize,
    Serialize,
};
use std_ext::result::ResultExt as _;
use thiserror::Error;

use super::{
    storage::{self, ReadOnlyStorage, Storage},
    tracking,
    types::{Namespace, Reference, RefsCategory},
};
use crate::{PeerId, Signature, Signer};

pub use crate::identities::git::Urn;
pub use git_ext::Oid;

/// The depth of the tracking graph (ie. [`Remotes`]) to retain per peer.
// TODO(kim): bubble up as parameter
pub const TRACKING_GRAPH_DEPTH: usize = 3;

/// The transitive tracking graph.
// **NOTE**: A recursion limit of 128 is imposed by `serde_json` when deserialising.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Remotes<A: Ord>(BTreeMap<A, Box<Remotes<A>>>);

impl<A: Ord> From<BTreeMap<A, Box<Remotes<A>>>> for Remotes<A> {
    fn from(bm: BTreeMap<A, Box<Remotes<A>>>) -> Self {
        Self(bm)
    }
}

impl<A> Default for Remotes<A>
where
    A: Default + Ord,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<A: Ord> Deref for Remotes<A> {
    type Target = BTreeMap<A, Box<Remotes<A>>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<A: Ord> DerefMut for Remotes<A> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<A: Ord> FromIterator<A> for Remotes<A> {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = A>,
    {
        Self(
            iter.into_iter()
                .map(|a| (a, Box::new(Self::new())))
                .collect(),
        )
    }
}

impl<A: Ord> Remotes<A> {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Build a new `self` with at most `depth` levels.
    pub fn cutoff(self, depth: usize) -> Self {
        if depth == 0 {
            return Self(BTreeMap::default());
        }

        Self(self.0.into_iter().fold(BTreeMap::new(), |mut acc, (k, v)| {
            acc.insert(k, Box::new(v.cutoff(depth - 1)));
            acc
        }))
    }

    /// Modify `self` to contain at most `depth` levels.
    pub fn cutoff_mut(&mut self, depth: usize) {
        if depth > 0 {
            let depth = depth - 1;
            for v in self.0.values_mut() {
                v.cutoff_mut(depth)
            }
        } else {
            self.clear()
        }
    }

    /// Traverse the tracking graph, yielding all nodes (of type `A`).
    ///
    /// Note that equal values of `A` may appear in the iterator, depending on
    /// the graph topology. To obtain the set of tracked `A`,
    /// [`Iterator::collect`] into a set type.
    pub fn flatten(&self) -> impl Iterator<Item = &A> {
        Flatten {
            outer: self.iter(),
            inner: None,
        }
    }
}

/// Iterator which yields all `A`s in an unspecified order.
struct Flatten<'a, A: Ord> {
    outer: btree_map::Iter<'a, A, Box<Remotes<A>>>,
    inner: Option<Box<Flatten<'a, A>>>,
}

impl<'a, A: Ord> Iterator for Flatten<'a, A> {
    type Item = &'a A;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(mut flat) = self.inner.take() {
            if let Some(a) = flat.next() {
                self.inner = Some(flat);
                return Some(a);
            }
        }

        if let Some((k, v)) = self.outer.next() {
            self.inner = Some(Box::new(Flatten {
                outer: v.iter(),
                inner: None,
            }));
            return Some(k);
        }

        None
    }
}

pub mod signing {
    use super::*;
    use std::error;

    #[derive(Debug, Error)]
    #[non_exhaustive]
    pub enum Error {
        #[error(transparent)]
        Sign(#[from] Box<dyn error::Error + Send + Sync + 'static>),
        #[error(transparent)]
        Cjson(#[from] CjsonError),
    }
}

pub mod stored {
    use super::*;

    pub(super) const BLOB_PATH: &str = "refs"; // `Path::new` ain't no const fn :(

    #[derive(Debug, Error)]
    #[non_exhaustive]
    pub enum Error {
        #[error(transparent)]
        Signed(#[from] signed::Error),

        #[error(transparent)]
        Signing(#[from] signing::Error),

        #[error(transparent)]
        Refname(#[from] reference::name::Error),

        #[error(transparent)]
        Json(#[from] serde_json::Error),

        #[error(transparent)]
        Cjson(#[from] CjsonError),

        #[error(transparent)]
        Store(#[from] storage::Error),

        #[error(transparent)]
        Git(#[from] git2::Error),

        #[error(transparent)]
        Tracked(#[from] tracking::error::TrackedPeers),
    }
}

/// Success result of [`Refs::update`]
pub enum Updated {
    /// The computed [`Refs`] were stored as a new commit.
    Updated { refs: Refs, at: git2::Oid },
    /// The stored [`Refs`] were the same as the computed ones, so no new commit
    /// was created.
    Unchanged { refs: Refs, at: git2::Oid },
    /// Another process committed [`Refs`], so the computed ones were discarded.
    ///
    /// This should typically be treated as a warning for interactive updates.
    /// See [0], [1] for further discussion.
    ///
    /// [0]: https://github.com/radicle-dev/radicle-link/pull/777
    /// [1]: https://lists.sr.ht/~radicle-link/dev/%3C20210830224202.GE10879%40schmidt.localdomain%3E
    ConcurrentlyModified,
}

/// The published state of a local repository.
#[derive(Clone, Debug, PartialEq)]
pub struct Refs {
    /// The signed references
    pub categorised_refs: BTreeMap<String, BTreeMap<String, Oid>>,

    /// The [`Remotes`], ie. tracking graph.
    ///
    /// Note that this does does not include the oids, as they can be determined
    /// by inspecting the `rad/signed_refs` of the respective remote.
    pub remotes: Remotes<PeerId>,
}

impl Refs {
    /// Compute the [`Refs`] from the current storage state at [`Urn`].
    #[tracing::instrument(level = "debug", skip(storage, urn), fields(urn = %urn))]
    pub fn compute<S>(storage: &S, urn: &Urn) -> Result<Self, stored::Error>
    where
        S: AsRef<storage::ReadOnly>,
    {
        let storage = storage.as_ref();
        let namespace = Namespace::from(urn);
        let namespace_prefix = format!("refs/namespaces/{}/", namespace);

        let peeled = |head: Result<git2::Reference, _>| -> Option<(String, git2::Oid)> {
            head.ok().and_then(reference::peeled)
        };

        let mut categorised_refs = BTreeMap::new();
        let glob = globset::Glob::new(format!("{}*", namespace_prefix).as_str())
            .unwrap()
            .compile_matcher();
        for (category, reference, oid) in storage
            .references_glob(glob)?
            .filter_map(peeled)
            .filter_map(|(r, oid)| {
                r.strip_prefix(&namespace_prefix)
                    .map(|s| (s.to_string(), oid))
            })
            .filter_map(|(ref_str, oid)| {
                ref_str
                    .parse::<reference::RefLike>()
                    .ok()
                    .map(reference::Qualified::from)
                    .and_then(|q| {
                        let (reference, category) = reference::OneLevel::from_qualified(q);
                        category.and_then(|c| {
                            let category: RefsCategory = c.into();
                            if ref_str.starts_with("refs/remotes")
                                || (RefsCategory::Rad == category
                                    && ref_str.ends_with("rad/signed_refs"))
                            {
                                None
                            } else {
                                Some((category.to_string(), reference, oid))
                            }
                        })
                    })
            })
        {
            let cat = categorised_refs
                .entry(category.to_string())
                .or_insert_with(BTreeMap::new);
            cat.insert(reference.to_string(), oid.into());
        }

        // Older librad implementations _always_ serialize the default git categories
        // and will throw an error if these categories are not present in the
        // signed refs, even if the repository contains no refs in those
        // categories. By adding empty maps for those categories here we
        // maintain backwards compatibility.
        for default_category in RefsCategory::default_categories() {
            if !categorised_refs.contains_key(default_category.to_string().as_str()) {
                categorised_refs.insert(default_category.to_string(), BTreeMap::new());
            }
        }

        let mut remotes =
            tracking::tracked_peers(storage, Some(urn))?.collect::<Result<Remotes<PeerId>, _>>()?;

        for (peer, tracked) in remotes.iter_mut() {
            if let Some(refs) = Self::load(storage, urn, *peer)? {
                *tracked = Box::new(refs.remotes.cutoff(TRACKING_GRAPH_DEPTH));
            }
        }

        Ok(Self {
            categorised_refs,
            remotes,
        })
    }

    /// Load the [`Refs`] of [`Urn`] (and optionally a remote `peer`) from
    /// storage, and verify the signature.
    ///
    /// If `peer` is `None`, the storage's [`PeerId`] is used for signature
    /// verification.
    ///
    /// If the blob where the signed [`Refs`] are expected to be stored is not
    /// found, `None` is returned.
    #[tracing::instrument(level = "debug", skip(storage, urn), fields(urn = %urn))]
    pub fn load<S, P>(storage: &S, urn: &Urn, peer: P) -> Result<Option<Self>, stored::Error>
    where
        S: AsRef<storage::ReadOnly>,
        P: Into<Option<PeerId>> + Debug,
    {
        let peer = peer.into();
        load(storage, urn, peer.as_ref()).map(|may| may.map(|Loaded { refs, .. }| Self::from(refs)))
    }

    /// Compute the current [`Refs`], sign them, and store them at the
    /// `rad/signed_refs` branch of [`Urn`].
    #[tracing::instrument(skip(storage, urn), fields(urn = %urn, local_peer = %storage.peer_id()))]
    pub fn update(storage: &Storage, urn: &Urn) -> Result<Updated, stored::Error> {
        let branch = Reference::rad_signed_refs(Namespace::from(urn), None);
        tracing::debug!("updating signed refs for {}", branch);

        let signed_refs = Self::compute(storage, urn)?.sign(storage.signer())?;

        let raw_git = storage.as_raw();

        let parent: Option<git2::Commit> = storage
            .reference(&branch)?
            .map(|r| r.peel_to_commit())
            .transpose()?;
        let tree = {
            let blob_oid = {
                let json = serde_json::to_vec(&signed_refs)?;
                raw_git.blob(&json)?
            };

            let mut builder = raw_git.treebuilder(None)?;
            builder.insert(stored::BLOB_PATH, blob_oid, 0o100_644)?;
            let oid = builder.write()?;

            raw_git.find_tree(oid)
        }?;

        if let Some(ref parent) = parent {
            if parent.tree()?.id() == tree.id() {
                return Ok(Updated::Unchanged {
                    refs: signed_refs.refs,
                    at: parent.id(),
                });
            }
        }

        let author = raw_git.signature()?;
        let commit = raw_git.commit(
            Some(reference::RefLike::from(&branch).as_str()),
            &author,
            &author,
            &format!("Update rad/signed_refs for {}", urn),
            &tree,
            &parent.iter().collect::<Vec<&git2::Commit>>(),
        );
        match commit {
            Ok(commit_id) => {
                tracing::trace!(
                    ?signed_refs.refs,
                    %branch,
                    head = %commit_id,
                    parent = ?parent.as_ref().map(|commit| commit.id()),
                    "updated signed refs for {}", urn
                );

                Ok(Updated::Updated {
                    refs: signed_refs.refs,
                    at: commit_id,
                })
            },
            Err(e) => match (e.class(), e.code()) {
                (git2::ErrorClass::Object, git2::ErrorCode::Modified) => {
                    Ok(Updated::ConcurrentlyModified)
                },
                _ => Err(e.into()),
            },
        }
    }

    pub fn sign<S>(self, signer: &S) -> Result<Signed<Verified>, signing::Error>
    where
        S: Signer,
    {
        let signature = futures::executor::block_on(signer.sign(&self.canonical_form()?))
            .map_err(|err| signing::Error::Sign(Box::new(err)))?;
        Ok(Signed {
            refs: self,
            signature: signature.into(),
            _verified: PhantomData,
        })
    }

    /// Iterator over all non-remote refs and their targets, paired with their
    /// corresponding `RefsCategory`.
    pub fn iter_categorised(
        &self,
    ) -> impl Iterator<Item = ((reference::OneLevel, &Oid), RefsCategory)> {
        let Refs {
            categorised_refs,
            remotes: _,
        } = self;
        categorised_refs
            .iter()
            .filter_map(|(category_str, refs)| {
                category_str.parse::<RefsCategory>().ok().map(|c| (c, refs))
            })
            .flat_map(move |(c, refs)| {
                refs.iter().filter_map(move |(ref_str, oid)| {
                    ref_str
                        .parse::<reference::RefLike>()
                        .ok()
                        .map(|r| ((r.into(), oid), c.clone()))
                })
            })
    }

    fn canonical_form(&self) -> Result<Vec<u8>, CjsonError> {
        Cjson(self).canonical_form()
    }

    fn refs_for_category(
        &self,
        category: RefsCategory,
    ) -> impl Iterator<Item = (reference::OneLevel, Oid)> + '_ {
        self.categorised_refs
            .get(&category.to_string())
            .into_iter()
            .flat_map(|refs| {
                refs.iter().filter_map(|(r, oid)| {
                    r.parse::<reference::RefLike>()
                        .ok()
                        .map(|r| (r.into(), *oid))
                })
            })
    }

    /// References under 'refs/heads'
    pub fn heads(&self) -> impl Iterator<Item = (reference::OneLevel, Oid)> + '_ {
        self.refs_for_category(RefsCategory::Heads)
    }

    /// References under 'refs/rad'
    pub fn rad(&self) -> impl Iterator<Item = (reference::OneLevel, Oid)> + '_ {
        self.refs_for_category(RefsCategory::Rad)
    }

    /// References under 'refs/tags'
    pub fn tags(&self) -> impl Iterator<Item = (reference::OneLevel, Oid)> + '_ {
        self.refs_for_category(RefsCategory::Tags)
    }

    /// References under 'refs/notes'
    pub fn notes(&self) -> impl Iterator<Item = (reference::OneLevel, Oid)> + '_ {
        self.refs_for_category(RefsCategory::Notes)
    }

    /// References under 'refs/cobs'
    pub fn cobs(&self) -> impl Iterator<Item = (reference::OneLevel, Oid)> + '_ {
        self.refs_for_category(RefsCategory::Cobs)
    }

    /// References where we don't know the category
    ///
    /// Returns an iterator of (category, reference, oid)
    pub fn other_refs(&self) -> impl Iterator<Item = (&str, &str, Oid)> {
        self.categorised_refs
            .iter()
            .filter(|(c, _)| {
                c.parse::<RefsCategory>()
                    .ok()
                    .map(|c| matches!(c, RefsCategory::Unknown(_)))
                    .unwrap_or(false)
            })
            .flat_map(|(category, references)| {
                references
                    .iter()
                    .map(move |(reference, oid)| (category.as_str(), reference.as_str(), *oid))
            })
    }
}

impl<V> From<Signed<V>> for Refs {
    fn from(sig: Signed<V>) -> Self {
        sig.refs
    }
}

pub mod signed {
    use super::*;

    #[derive(Debug, Error)]
    #[non_exhaustive]
    pub enum Error {
        #[error("invalid signature")]
        InvalidSignature(Refs),

        #[error(transparent)]
        Json(#[from] serde_json::error::Error),

        #[error(transparent)]
        Cjson(#[from] CjsonError),
    }
}

/// Type witness to tell us that a [`Signed`] is in a verified state.
pub struct Verified;

/// Type witness to tell us that a [`Signed`] is in a unverified state.
pub struct Unverified;

/// `Signed` is the combination of [`Refs`] and a [`Signature`]. The `Signature`
/// is cryptographic signature over the `Refs`. This allows us to easily verify
/// if a set of `Refs` came from a particular [`PeerId`].
///
/// The type parameter keeps track of whether the `Signed` was [`Verified`] or
/// [`Unverified`].
///
/// The only way to produce a [`Signed`] that is verified is either by verifying
/// [`Refs`] with a [`Signer`], or verifying an unverified set with a
/// [`PeerId`], using [`Signed::verify`]. A shorthand for verifying bytes with a
/// `PeerId` is given by [`Signed::from_json`].
///
/// Note that we may only persist a `Signed<Verified>`, and can only deserialize
/// a `Signed<Unverified>`.
pub struct Signed<V> {
    refs: Refs,
    signature: Signature,
    _verified: PhantomData<V>,
}

impl Signed<Verified> {
    pub fn from_json(data: &[u8], signer: &PeerId) -> Result<Self, signed::Error> {
        let unknown = serde_json::from_slice(data)?;
        Self::verify(unknown, signer)
    }

    pub fn verify(unknown: Signed<Unverified>, signer: &PeerId) -> Result<Self, signed::Error> {
        let canonical = unknown.refs.canonical_form()?;
        if unknown.signature.verify(&canonical, &**signer) {
            Ok(Signed {
                refs: unknown.refs,
                signature: unknown.signature,
                _verified: PhantomData,
            })
        } else {
            Err(signed::Error::InvalidSignature(unknown.refs))
        }
    }
}

impl<V> Deref for Signed<V> {
    type Target = Refs;

    fn deref(&self) -> &Self::Target {
        &self.refs
    }
}

impl<'de> Deserialize<'de> for Signed<Unverified> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        const SIGNATURE: &str = "Signature";
        const FIELD_REFS: &str = "refs";
        const FIELD_SIGNATURE: &str = "signature";

        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "lowercase")]
        enum Field {
            Refs,
            Signature,
        }

        struct SignedVisitor;

        impl<'de> de::Visitor<'de> for SignedVisitor {
            type Value = Signed<Unverified>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct Signed")
            }

            fn visit_map<V>(self, mut map: V) -> Result<Signed<Unverified>, V::Error>
            where
                V: de::MapAccess<'de>,
            {
                let mut refs = None;
                let mut signature = None;
                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Refs => {
                            if refs.is_some() {
                                return Err(de::Error::duplicate_field(FIELD_REFS));
                            }
                            refs = Some(map.next_value()?);
                        },
                        Field::Signature => {
                            if signature.is_some() {
                                return Err(de::Error::duplicate_field(FIELD_SIGNATURE));
                            }
                            signature = Some(map.next_value()?);
                        },
                    }
                }
                let refs = refs.ok_or_else(|| de::Error::missing_field(FIELD_REFS))?;
                let signature =
                    signature.ok_or_else(|| de::Error::missing_field(FIELD_SIGNATURE))?;
                Ok(Signed {
                    refs,
                    signature,
                    _verified: PhantomData,
                })
            }
        }

        const FIELDS: &[&str] = &[FIELD_REFS, FIELD_SIGNATURE];
        deserializer.deserialize_struct(SIGNATURE, FIELDS, SignedVisitor)
    }
}

impl Serialize for Signed<Verified> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        let mut state = serializer.serialize_struct("Signed", 2)?;
        state.serialize_field("refs", &self.refs)?;
        state.serialize_field("signature", &self.signature)?;
        state.end()
    }
}

pub(crate) struct Loaded {
    #[allow(unused)]
    pub at: git_ext::Oid,
    pub refs: Signed<Verified>,
}

pub(crate) fn load<S>(
    storage: S,
    urn: &Urn,
    peer: Option<&PeerId>,
) -> Result<Option<Loaded>, stored::Error>
where
    S: AsRef<storage::ReadOnly>,
{
    let sigrefs = Reference::rad_signed_refs(Namespace::from(urn), peer.copied());
    let tip = storage
        .as_ref()
        .reference_oid(&sigrefs)
        .map(Some)
        .or_matches(
            |e| matches!(e, storage::read::Error::Git(e) if is_not_found_err(e)),
            || Ok::<_, storage::read::Error>(None),
        )?;
    match tip {
        None => Ok(None),
        Some(at) => {
            tracing::debug!("loading signed_refs from {}:{}", &sigrefs, &at);
            load_at(storage, at, peer)
        },
    }
}

pub(crate) fn load_at<S>(
    storage: S,
    at: git_ext::Oid,
    peer: Option<&PeerId>,
) -> Result<Option<Loaded>, stored::Error>
where
    S: AsRef<storage::ReadOnly>,
{
    let signer = peer.unwrap_or_else(|| storage.as_ref().peer_id());
    let loaded = storage
        .as_ref()
        .blob_at(at, Path::new(stored::BLOB_PATH))?
        .map(|blob| Signed::from_json(blob.content(), signer))
        .transpose()
        .map_err(stored::Error::from)?
        .map(|refs| Loaded { at, refs });

    Ok(loaded)
}
