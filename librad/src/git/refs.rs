// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::{btree_map, BTreeMap},
    convert::TryFrom,
    fmt::{self, Debug},
    iter::FromIterator,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    path::Path,
};

use git_ext::reference;
use serde::{
    de,
    ser::{self, SerializeStruct},
    Deserialize,
    Serialize,
};
use thiserror::Error;

use super::{
    storage::{self, ReadOnlyStorage, Storage},
    tracking,
    types::{Namespace, Reference, RefsCategory},
};
use crate::{
    internal::canonical::{Cjson, CjsonError},
    PeerId,
    Signature,
    Signer,
};

pub use crate::identities::git::Urn;
pub use git_ext::Oid;

/// The depth of the tracking graph (ie. [`Remotes`]) to retain per peer.
// TODO(kim): bubble up as parameter
pub const TRACKING_GRAPH_DEPTH: usize = 3;

/// The transitive tracking graph.
// **NOTE**: A recursion limit of 128 is imposed by `serde_json` when deserialising.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Remotes<A: Ord>(BTreeMap<A, Box<Remotes<A>>>);

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
        Track(#[from] tracking::Error),

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
    }
}

/// The published state of a local repository.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Refs {
    /// `refs/heads/*`
    pub heads: BTreeMap<reference::OneLevel, Oid>,

    /// `refs/rad/*`, excluding `refs/rad/signed_refs`
    pub rad: BTreeMap<reference::OneLevel, Oid>,

    /// `refs/tags/*`
    pub tags: BTreeMap<reference::OneLevel, Oid>,

    /// `refs/notes/*`
    pub notes: BTreeMap<reference::OneLevel, Oid>,

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

        fn peeled(r: Result<git2::Reference, storage::Error>) -> Option<(String, git2::Oid)> {
            r.ok().and_then(|head| {
                head.name()
                    .and_then(|name| head.target().map(|target| (name.to_owned(), target)))
            })
        }

        let refined = |(name, oid): (String, git2::Oid)|
             -> Result<(reference::OneLevel, Oid), stored::Error>
        {
            let name = reference::RefLike::try_from(
                name.strip_prefix(&namespace_prefix).unwrap_or(&name)
            )?;
            Ok((reference::OneLevel::from(name), oid.into()))
        };

        let heads = storage
            .references(&Reference::heads(namespace.clone(), None))?
            .filter_map(peeled)
            .map(refined)
            .collect::<Result<_, _>>()?;
        let rad = storage
            .references(&Reference::rads(namespace.clone(), None))?
            .filter_map(peeled)
            .filter(|(name, _)| !name.ends_with("rad/signed_refs"))
            .map(refined)
            .collect::<Result<_, _>>()?;
        let tags = storage
            .references(&Reference::tags(namespace.clone(), None))?
            .filter_map(peeled)
            .map(refined)
            .collect::<Result<_, _>>()?;
        let notes = storage
            .references(&Reference::notes(namespace, None))?
            .filter_map(peeled)
            .map(refined)
            .collect::<Result<_, _>>()?;

        let mut remotes = tracking::tracked(storage, urn)?.collect::<Remotes<PeerId>>();
        for (peer, tracked) in remotes.iter_mut() {
            if let Some(refs) = Self::load(storage, urn, *peer)? {
                *tracked = Box::new(refs.remotes.cutoff(TRACKING_GRAPH_DEPTH));
            }
        }

        Ok(Self {
            heads,
            rad,
            tags,
            notes,
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
    #[tracing::instrument(skip(storage, urn), fields(urn = %urn))]
    pub fn load<S, P>(storage: &S, urn: &Urn, peer: P) -> Result<Option<Self>, stored::Error>
    where
        S: AsRef<storage::ReadOnly>,
        P: Into<Option<PeerId>> + Debug,
    {
        let storage = storage.as_ref();
        let peer = peer.into();
        let signer = peer.unwrap_or_else(|| *storage.peer_id());

        let blob_ref = Reference::rad_signed_refs(Namespace::from(urn), peer);
        let blob_path = Path::new(stored::BLOB_PATH);

        tracing::debug!(
            "loading signed_refs from {} {}",
            &blob_ref,
            blob_path.display()
        );

        let maybe_blob = storage.blob(&blob_ref, blob_path)?;
        maybe_blob
            .map(|blob| Signed::from_json(blob.content(), &signer).map(|signed| signed.refs))
            .transpose()
            .map_err(stored::Error::from)
    }

    /// Compute the current [`Refs`], sign them, and store them at the
    /// `rad/signed_refs` branch of [`Urn`].
    ///
    /// If the result of [`Self::compute`] is the same as the alread-stored
    /// [`Refs`], no commit is made and `None` is returned. Otherwise, the
    /// new and persisted [`Refs`] are returned in a `Some`.
    #[tracing::instrument(skip(storage, urn), fields(urn = %urn))]
    pub fn update(storage: &Storage, urn: &Urn) -> Result<Option<Self>, stored::Error> {
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
                tracing::debug!("signed refs already up-to-date");
                return Ok(None);
            }
        }

        let commit_id = {
            let author = raw_git.signature()?;
            raw_git.commit(
                Some(reference::RefLike::from(&branch).as_str()),
                &author,
                &author,
                &format!("Update rad/signed_refs for {}", urn),
                &tree,
                &parent.iter().collect::<Vec<&git2::Commit>>(),
            )?
        };
        tracing::trace!(
            "updated signed refs at {} to {}: {:?}",
            branch,
            commit_id,
            signed_refs.refs
        );

        Ok(Some(signed_refs.refs))
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
    ) -> impl Iterator<Item = ((&reference::OneLevel, &Oid), RefsCategory)> {
        let Refs {
            heads,
            rad,
            tags,
            notes,
            remotes: _,
        } = self;
        heads
            .iter()
            .map(|x| (x, RefsCategory::Heads))
            .chain(rad.iter().map(|x| (x, RefsCategory::Rad)))
            .chain(tags.iter().map(|x| (x, RefsCategory::Tags)))
            .chain(notes.iter().map(|x| (x, RefsCategory::Notes)))
    }

    fn canonical_form(&self) -> Result<Vec<u8>, CjsonError> {
        Cjson(self).canonical_form()
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
        if unknown.signature.verify(&canonical, &*signer) {
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
