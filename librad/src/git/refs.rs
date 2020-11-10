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
    collections::BTreeMap,
    convert::TryFrom,
    fmt::{self, Debug},
    hash::Hash,
    iter,
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
    storage::{self, Storage},
    tracking,
    types::{namespace::Namespace, NamespacedRef},
};
use crate::{
    internal::canonical::{Cjson, CjsonError},
    keys::Signature,
    peer::PeerId,
    signer::Signer,
};

pub use crate::identities::git::Urn;
pub use git_ext::Oid;

/// The transitive tracking graph, up to 3 degrees
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Remotes<A: PartialEq + Eq + Ord>(BTreeMap<A, BTreeMap<A, BTreeMap<A, ()>>>);

impl<A> Remotes<A>
where
    A: PartialEq + Eq + Ord + Hash,
{
    pub fn cutoff(self) -> BTreeMap<A, BTreeMap<A, ()>>
    where
        A: Clone,
    {
        self.0
            .into_iter()
            .map(|(k, v)| (k, v.keys().map(|x| (x.clone(), ())).collect()))
            .collect()
    }

    pub fn flatten(&self) -> impl Iterator<Item = &A> {
        self.0.iter().flat_map(|(k, v)| {
            iter::once(k).chain(
                v.iter()
                    .flat_map(|(k1, v1)| iter::once(k1).chain(v1.keys())),
            )
        })
    }

    pub fn from_map(map: BTreeMap<A, BTreeMap<A, BTreeMap<A, ()>>>) -> Self {
        Self(map)
    }

    pub fn boxed(self) -> Box<Self> {
        Box::new(self)
    }
}

impl<A> Deref for Remotes<A>
where
    A: PartialEq + Eq + Ord + Hash,
{
    type Target = BTreeMap<A, BTreeMap<A, BTreeMap<A, ()>>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<A> DerefMut for Remotes<A>
where
    A: PartialEq + Eq + Ord + Hash,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<A> From<BTreeMap<A, BTreeMap<A, BTreeMap<A, ()>>>> for Remotes<A>
where
    A: PartialEq + Eq + Ord + Hash,
{
    fn from(map: BTreeMap<A, BTreeMap<A, BTreeMap<A, ()>>>) -> Self {
        Self::from_map(map)
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

/// The current `refs/heads` and [`Remotes`] (transitive tracking graph)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Refs {
    pub heads: BTreeMap<reference::OneLevel, Oid>,
    pub remotes: Remotes<PeerId>,
}

impl Refs {
    /// Compute the [`Refs`] from the current storage state at [`Urn`].
    #[tracing::instrument(level = "debug", skip(storage), err)]
    pub fn compute<S>(storage: &Storage<S>, urn: &Urn) -> Result<Self, stored::Error>
    where
        S: Signer,
    {
        let namespace = Namespace::from(urn);
        let namespace_prefix = format!("refs/namespaces/{}/", namespace);
        let heads_ref = NamespacedRef::heads(namespace, None);

        tracing::debug!("reading heads from {}", &heads_ref);

        let heads = storage
            .references(&heads_ref)?
            // FIXME: this is `git_ext::reference::iter::References::peeled()`,
            // which we need to generalise to allow impl Iterator combinators
            .filter_map(|reference| {
                reference.ok().and_then(|head| {
                    head.name()
                        .and_then(|name| head.target().map(|target| (name.to_owned(), target)))
                })
            })
            .try_fold(BTreeMap::new(), |mut acc, (name, oid)| {
                tracing::trace!("raw refname: {}", name);
                let name = name.strip_prefix(&namespace_prefix).unwrap_or(&name);
                tracing::trace!("stripped namespace: {}", name);
                let refname = reference::RefLike::try_from(name)?;
                acc.insert(reference::OneLevel::from(refname), oid.into());

                Ok::<_, stored::Error>(acc)
            })?;

        let mut remotes = tracking::tracked(storage, urn)?
            .map(|peer| (peer, BTreeMap::new()))
            .collect::<BTreeMap<PeerId, BTreeMap<PeerId, BTreeMap<PeerId, ()>>>>();

        for (peer, tracked) in remotes.iter_mut() {
            if let Some(refs) = Self::load(storage, urn, *peer)? {
                *tracked = refs.remotes.cutoff();
            }
        }

        Ok(Self {
            heads,
            remotes: remotes.into(),
        })
    }

    /// Load the [`Refs`] of [`Urn`] (and optionally a remote `peer`) from
    /// storage, and verify the signature.
    ///
    /// If `peer` is `None`, the signer's public key is used for signature
    /// verification.
    ///
    /// If the blob where the signed [`Refs`] are expected to be stored is not
    /// found, `None` is returned.
    #[tracing::instrument(skip(storage), err)]
    pub fn load<S, P>(
        storage: &Storage<S>,
        urn: &Urn,
        peer: P,
    ) -> Result<Option<Self>, stored::Error>
    where
        S: Signer,
        P: Into<Option<PeerId>> + Debug,
    {
        let peer = peer.into();
        let signer = peer.unwrap_or_else(|| PeerId::from_signer(storage.signer()));

        let blob_ref = NamespacedRef::rad_signed_refs(Namespace::from(urn), peer);
        let blob_path = Path::new(stored::BLOB_PATH);

        tracing::debug!(
            "loading signed_refs from {} {}",
            &blob_ref,
            blob_path.display()
        );

        let maybe_blob = storage.blob(&blob_ref, &blob_path)?;
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
    #[tracing::instrument(skip(storage), err)]
    pub fn update<S>(storage: &Storage<S>, urn: &Urn) -> Result<Option<Self>, stored::Error>
    where
        S: Signer,
    {
        let branch = NamespacedRef::rad_signed_refs(Namespace::from(urn), None);
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
