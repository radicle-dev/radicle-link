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
    collections::{HashMap, HashSet},
    convert::{Into, TryFrom},
    marker::PhantomData,
    str::FromStr,
};

use serde::{
    de::{DeserializeOwned, Error as SerdeDeserializationError},
    Deserialize,
    Serialize,
};
use thiserror::Error;

use crate::{
    hash::{Hash, ParseError as HashParseError},
    keys::{PublicKey, SecretKey, Signature},
    meta::user::User,
    uri::{Path, Protocol, RadUrn},
};

pub mod data;

use data::{EntityData, EntityInfo, EntityInfoExt, EntityKind};

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum Error {
    #[error("Serialization failed ({0})")]
    SerializationFailed(String),

    #[error("Invalid UTF8 ({0})")]
    InvalidUtf8(String),

    #[error("Invalid buffer encoding ({0})")]
    InvalidBufferEncoding(String),

    #[error("Invalid hash ({0})")]
    InvalidHash(String),

    #[error("Wrong hash (claimed {claimed:?}, actual {actual:?})")]
    WrongHash { claimed: String, actual: String },

    #[error("Hash parse error ({0})")]
    HashParseError(#[from] HashParseError),

    #[error("Invalid root hash")]
    InvalidRootHash,

    #[error("Missing root hash")]
    MissingRootHash,

    #[error("Invalid URI ({0})")]
    InvalidUri(String),

    #[error("Signature already present ({0})")]
    SignatureAlreadyPresent(PublicKey),

    #[error("Invalid data ({0})")]
    InvalidData(String),

    #[error("Builder error ({0})")]
    BuilderError(&'static str),

    #[error("Key not present ({0})")]
    KeyNotPresent(PublicKey),

    #[error("User key not present (uri {0}, key {1})")]
    UserKeyNotPresent(RadUrn, PublicKey),

    #[error("Signature missing")]
    SignatureMissing,

    #[error("Signature decoding failed")]
    SignatureDecodingFailed,

    #[error("Signature verification failed")]
    SignatureVerificationFailed,

    #[error("Resolution failed ({0})")]
    ResolutionFailed(RadUrn),

    #[error("Resolution at revision failed ({0}, revision {1})")]
    RevisionResolutionFailed(RadUrn, u64),
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum UpdateVerificationError {
    #[error("Non monotonic revision")]
    NonMonotonicRevision,

    #[error("Wrong parent hash")]
    WrongParentHash,

    #[error("Wrong root hash")]
    WrongRootHash,

    #[error("Update without previous quorum")]
    NoPreviousQuorum,

    #[error("Update without current quorum")]
    NoCurrentQuorum,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum HistoryVerificationError {
    #[error("Empty history")]
    EmptyHistory,

    #[error("Error at revsion (rev {revision:?}, err {error:?})")]
    ErrorAtRevision { revision: u64, error: Error },

    #[error("Update error (rev {revision:?}, err {error:?})")]
    UpdateError {
        revision: u64,
        error: UpdateVerificationError,
    },
}

/// Type witness for a fully verified [`Entity`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified;

/// Type witness for a signed [`Entity`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signed;

/// Type witness for a draft [`Entity`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft;

/// A type expressing *who* is signing an `Entity`
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Signatory {
    /// A specific user (identified by their URN)
    User(RadUrn),
    /// The entity itself (with an owned key)
    OwnedKey,
}

/// A signature for an `Entity``
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntitySignature {
    /// Who is producing this signature
    pub by: Signatory,
    /// The signature data
    pub sig: Signature,
}

/// An URN resolver that turns URNs into `Entity` instances
/// (`T` is the entity type)
#[async_trait]
pub trait Resolver<T> {
    /// Resolve the given URN and deserialize the target `Entity`
    async fn resolve(&self, uri: &RadUrn) -> Result<T, Error>;
    async fn resolve_revision(&self, uri: &RadUrn, revision: u64) -> Result<T, Error>;
}

/// The base entity definition.
///
/// Entities have the following properties:
///
/// - They can evolve over time, so they have a sequence of revisions.
/// - Their identity is stable (it does not change over time), and it is the
///   hash of their initial revision.
/// - Each revision contains the hash of the previous revision, which is also
///   hashed, so that the sequence of revisions is a Merkle tree (actually just
///   a list).
/// - They can be signed, either with a key they own, or using a key belonging
///   to a different entity (the certifier); note that when applying multiple
///   signatures, signatures are not themselves signed (what is signed is always
///   only the entity itself).
/// - Each revision specifies the set of owned keys and trusted certifiers.
/// - Each revision must be signed by all its owned keys and trusted certifiers.
/// - Each subsequent revision must be signed by a quorum of the previous keys
///   and certifiers, to prove that the entity evolution is actually under the
///   control of its current "owners" (the idea is taken from [TUF](https://theupdateframework.io/)).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entity<T, ST> {
    /// Verification status marker type
    status_marker: PhantomData<ST>,
    /// The entity name (useful for humans because the hash is unreadable)
    name: String,
    /// Entity revision, to be incremented at each entity update
    revision: u64,
    /// Radicle software version used to serialize the entity
    rad_version: u8,
    /// Entity hash, computed on everything except the signatures and
    /// the hash itself
    hash: Hash,
    /// Hash of the root of the revision Merkle tree (the entity ID)
    root_hash: Hash,
    /// Hash of the previous revision, `None` for the initial revision
    /// (in this case the entity hash is actually the entity ID)
    parent_hash: Option<Hash>,
    /// Set of signatures
    signatures: HashMap<PublicKey, EntitySignature>,
    /// Set of owned keys
    keys: HashSet<PublicKey>,
    /// Set of certifiers (entities identified by their URN)
    certifiers: HashSet<RadUrn>,
    /// Specific `Entity` data
    info: T,
}

impl<T> TryFrom<EntityData<T>> for Entity<T, Draft>
where
    T: Serialize + DeserializeOwned + Clone + EntityInfoExt,
{
    type Error = Error;
    fn try_from(data: EntityData<T>) -> Result<Entity<T, Draft>, Error> {
        Self::from_data(data)
    }
}

impl<T, ST> Into<EntityData<T>> for Entity<T, ST>
where
    T: Serialize + DeserializeOwned + Clone + EntityInfoExt,
    ST: Clone,
{
    fn into(self) -> EntityData<T> {
        self.to_data()
    }
}

impl<T, ST> Serialize for Entity<T, ST>
where
    T: Serialize + DeserializeOwned + Clone + EntityInfoExt,
    ST: Clone,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_data().serialize(serializer)
    }
}

impl<'de, T> Deserialize<'de> for Entity<T, Draft>
where
    T: Serialize + DeserializeOwned + Clone + EntityInfoExt,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
        D::Error: SerdeDeserializationError,
    {
        let data = EntityData::<T>::deserialize(deserializer)?;
        let res = Entity::<T, Draft>::try_from(data);
        res.map_err(D::Error::custom)
    }
}

impl<T, ST> Entity<T, ST>
where
    T: Serialize + DeserializeOwned + Clone + EntityInfoExt,
{
    /// `name` getter
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `revision` getter
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// `rad_version` getter
    pub fn rad_version(&self) -> u8 {
        self.rad_version
    }

    /// `info` getter
    pub fn info(&self) -> &T {
        &self.info
    }

    /// Gets the entity kind
    pub fn kind(&self) -> EntityKind {
        self.info.kind()
    }

    /// Turn the entity in to its raw data
    /// (first step of serialization and reverse of [`Entity::from_data`])
    pub fn to_data(&self) -> EntityData<T> {
        let mut signatures = HashMap::new();
        for (k, s) in self.signatures() {
            signatures.insert(
                k.to_owned(),
                data::EntitySignatureData {
                    user: match &s.by {
                        Signatory::User(uri) => Some(uri.to_string()),
                        Signatory::OwnedKey => None,
                    },
                    sig: s.sig.to_bs58(),
                },
            );
        }

        EntityData {
            name: Some(self.name.to_owned()),
            revision: Some(self.revision),
            rad_version: self.rad_version,
            hash: Some(self.hash.clone()),
            root_hash: Some(self.root_hash.clone()),
            parent_hash: self.parent_hash.clone(),
            signatures: Some(signatures),
            keys: self.keys.clone(),
            certifiers: self.certifiers.clone(),
            info: self.info.to_owned(),
        }
    }

    /// Helper to build a new entity cloning the current one
    /// (signatures are cleared because they would be invalid anyway)
    pub fn to_builder(&self) -> EntityData<T> {
        self.to_data().clear_hash().clear_signatures()
    }

    /// `hash` getter
    pub fn hash(&self) -> &Hash {
        &self.hash
    }

    /// `root_hash` getter
    pub fn root_hash(&self) -> &Hash {
        &self.root_hash
    }

    /// `urn` getter
    pub fn urn(&self) -> RadUrn {
        RadUrn::new(self.hash.to_owned(), Protocol::Git, Path::new())
    }

    /// `parent_hash` getter
    pub fn parent_hash(&self) -> &Option<Hash> {
        &self.parent_hash
    }

    /// `signatures` getter
    pub fn signatures(&self) -> &HashMap<PublicKey, EntitySignature> {
        &self.signatures
    }

    /// `keys` getter
    pub fn keys(&self) -> &HashSet<PublicKey> {
        &self.keys
    }
    /// Keys count
    fn keys_count(&self) -> usize {
        self.keys.len()
    }
    /// Check key presence
    fn has_key(&self, key: &PublicKey) -> bool {
        self.keys.contains(key)
    }

    /// `certifiers` getter
    pub fn certifiers(&self) -> &HashSet<RadUrn> {
        &self.certifiers
    }
    /// Certifiers count
    fn certifiers_count(&self) -> usize {
        self.certifiers.len()
    }
    /// Check certifier presence
    fn has_certifier(&self, c: &RadUrn) -> bool {
        self.certifiers.contains(c)
    }

    /// Turn the entity into its canonical data representation
    /// (for hashing or signing)
    pub fn canonical_data(&self) -> Result<Vec<u8>, Error> {
        self.to_data().canonical_data()
    }

    pub fn map<U, F>(self, f: F) -> Entity<U, ST>
    where
        F: FnOnce(T) -> U,
    {
        Entity {
            status_marker: self.status_marker,
            name: self.name,
            revision: self.revision,
            rad_version: self.rad_version,
            hash: self.hash,
            root_hash: self.root_hash,
            parent_hash: self.parent_hash,
            signatures: self.signatures,
            keys: self.keys,
            certifiers: self.certifiers,
            info: f(self.info),
        }
    }

    pub fn try_map<U, E, F>(self, f: F) -> Result<Entity<U, ST>, E>
    where
        F: FnOnce(T) -> Result<U, E>,
    {
        let info = f(self.info)?;
        Ok(Entity {
            status_marker: self.status_marker,
            name: self.name,
            revision: self.revision,
            rad_version: self.rad_version,
            hash: self.hash,
            root_hash: self.root_hash,
            parent_hash: self.parent_hash,
            signatures: self.signatures,
            keys: self.keys,
            certifiers: self.certifiers,
            info,
        })
    }

    /// Check that this key is allowed to sign the entity by checking that the
    /// `by` argument is correct:
    ///
    /// - either the key is owned by `self`
    /// - or it belongs to the given certifier
    pub async fn check_key(
        &self,
        key: &PublicKey,
        by: &Signatory,
        resolver: &impl Resolver<User<Draft>>,
    ) -> Result<(), Error> {
        match by {
            Signatory::OwnedKey => {
                if !self.has_key(key) {
                    return Err(Error::KeyNotPresent(key.to_owned()));
                }
            },
            Signatory::User(uri) => {
                let user = resolver.resolve(&uri).await?;
                if !user.has_key(key) {
                    return Err(Error::UserKeyNotPresent(uri.to_owned(), key.to_owned()));
                }
            },
        }
        Ok(())
    }

    /// Given a private key, compute the signature of the entity canonical data
    /// FIXME[ENTITY]: we should check the hash instead: it is cheaper and makes
    /// also verification way faster because we would not need to rebuild the
    /// canonical data at every check (we can trust the hash correctness)
    pub fn compute_signature(&self, key: &SecretKey) -> Result<Signature, Error> {
        Ok(key.sign(&self.canonical_data()?))
    }

    /// Given a private key owned by the entity, sign the current entity
    ///
    /// The following checks are performed:
    ///
    /// - the entity has not been already signed using this same key
    /// - this key is owned by the current entity
    pub fn sign_owned(&mut self, key: &SecretKey) -> Result<(), Error> {
        let public_key = key.public();
        if self.signatures().contains_key(&public_key) {
            return Err(Error::SignatureAlreadyPresent(public_key));
        }
        if !self.has_key(&public_key) {
            return Err(Error::KeyNotPresent(public_key));
        }
        let signature = EntitySignature {
            by: Signatory::OwnedKey,
            sig: self.compute_signature(key)?,
        };
        self.signatures.insert(public_key, signature);
        Ok(())
    }

    /// Given a private key owned by a verified user, sign the current entity
    ///
    /// The following checks are performed:
    ///
    /// - the entity has not been already signed using this same key
    /// - this key is owned by the provided user
    pub fn sign_by_user(&mut self, key: &SecretKey, user: &User<Verified>) -> Result<(), Error> {
        let public_key = key.public();
        if self.signatures().contains_key(&public_key) {
            return Err(Error::SignatureAlreadyPresent(public_key));
        }
        if !user.has_key(&public_key) {
            return Err(Error::UserKeyNotPresent(user.urn(), public_key));
        }
        let signature = EntitySignature {
            by: Signatory::User(user.urn()),
            sig: self.compute_signature(key)?,
        };
        self.signatures.insert(public_key, signature);
        Ok(())
    }

    /// Given a private key, sign the current entity
    ///
    /// The following checks are performed:
    ///
    /// - the entity has not been already signed using this same key
    /// - this key is allowed to sign the entity (using `check_key`)
    pub async fn sign(
        &mut self,
        key: &SecretKey,
        by: &Signatory,
        resolver: &impl Resolver<User<Draft>>,
    ) -> Result<(), Error> {
        let public_key = key.public();
        if self.signatures().contains_key(&public_key) {
            return Err(Error::SignatureAlreadyPresent(public_key.to_owned()));
        }
        self.check_key(&public_key, by, resolver).await?;
        let signature = EntitySignature {
            by: by.to_owned(),
            sig: self.compute_signature(key)?,
        };
        self.signatures.insert(public_key, signature);
        Ok(())
    }

    /// Check that an entity signature is valid
    pub async fn check_signature(
        &self,
        key: &PublicKey,
        by: &Signatory,
        signature: &Signature,
        resolver: &impl Resolver<User<Draft>>,
    ) -> Result<Entity<T, Signed>, Error>
    where
        ST: Clone,
    {
        self.check_key(key, by, resolver).await?;
        if signature.verify(&self.canonical_data()?, key) {
            Ok(self.clone().with_status::<Signed>())
        } else {
            Err(Error::SignatureVerificationFailed)
        }
    }

    fn with_status<NewSt>(self) -> Entity<T, NewSt> {
        Entity::<T, NewSt> {
            status_marker: PhantomData,
            name: self.name,
            revision: self.revision,
            rad_version: self.rad_version,
            hash: self.hash,
            root_hash: self.root_hash,
            parent_hash: self.parent_hash,
            keys: self.keys,
            certifiers: self.certifiers,
            signatures: self.signatures,
            info: self.info,
        }
    }

    /// Compute the status of this entity (only this revision is checked)
    ///
    /// This checks that:
    /// - every owned key and certifier has a corresponding signature
    /// - only owned keys and certifiers have signed the entity
    /// - the first revision has no parent and a matching root hash
    pub async fn check_signatures(
        self,
        resolver: &impl Resolver<User<Draft>>,
    ) -> Result<Entity<T, Signed>, Error>
    where
        ST: Clone,
    {
        let mut keys = self.keys().iter().cloned().collect::<HashSet<_>>();
        let mut users = self.certifiers().iter().cloned().collect::<HashSet<_>>();

        if self.revision == 1 && (self.parent_hash.is_some() || self.root_hash != self.hash) {
            // TODO: define a better error if `self.parent_hash.is_some()`
            // (should be "revision 1 cannot have a parent hash")
            return Err(Error::InvalidRootHash);
        }

        for (k, s) in self.signatures() {
            match self.check_signature(k, &s.by, &s.sig, resolver).await {
                Err(e) => return Err(e),
                Ok(_signed) => {},
            };

            match &s.by {
                Signatory::OwnedKey => {
                    keys.remove(k);
                },
                Signatory::User(user) => {
                    users.remove(&user);
                },
            }
        }
        if keys.is_empty() && users.is_empty() {
            Ok(self.with_status::<Signed>())
        } else {
            Err(Error::SignatureMissing)
        }
    }

    /// Given an entity and its previous revision check that the update is
    /// valid:
    ///
    /// - the revision has been incremented
    /// - the parent hash is correct
    /// - the root hash is correct
    /// - the TUF quorum rules have been observed
    ///
    /// FIXME[ENTITY]: only allow exact `+1`increments so that the revision
    /// history has no holes
    /// FIXME[ENTITY]: probably we should merge owned keys and certifiers when
    /// checking the quorum rules (now we are handling them separately)
    fn check_update<OtherST>(
        self,
        previous: Entity<T, OtherST>,
    ) -> Result<Entity<T, OtherST>, UpdateVerificationError> {
        if self.revision() <= previous.revision() {
            return Err(UpdateVerificationError::NonMonotonicRevision);
        }

        match &self.parent_hash {
            Some(parent_hash) => {
                if &previous.hash != parent_hash {
                    return Err(UpdateVerificationError::WrongParentHash);
                }
            },
            None => {
                return Err(UpdateVerificationError::WrongParentHash);
            },
        }

        if self.root_hash != previous.root_hash {
            return Err(UpdateVerificationError::WrongRootHash);
        }

        let retained_keys = self.keys().iter().filter(|k| previous.has_key(k)).count();
        let total_keys = self.keys_count();
        let added_keys = total_keys - retained_keys;
        let removed_keys = previous.keys_count() - retained_keys;
        let quorum_keys = total_keys / 2;

        if added_keys > quorum_keys {
            return Err(UpdateVerificationError::NoCurrentQuorum);
        } else if removed_keys > quorum_keys {
            return Err(UpdateVerificationError::NoPreviousQuorum);
        }

        let retained_certifiers = self
            .certifiers()
            .iter()
            .filter(|c| previous.has_certifier(c))
            .count();
        let total_certifiers = self.certifiers_count();
        let added_certifiers = total_certifiers - retained_certifiers;
        let removed_certifiers = previous.certifiers_count() - retained_certifiers;
        let quorum_certifiers = total_certifiers / 2;

        if added_certifiers > quorum_certifiers {
            return Err(UpdateVerificationError::NoCurrentQuorum);
        } else if removed_certifiers > quorum_certifiers {
            return Err(UpdateVerificationError::NoPreviousQuorum);
        }

        Ok(previous)
    }

    /// Compute the entity status checking that the whole revision history is
    /// valid
    ///
    /// FIXME[ENTITY]: should we allow certifiers that are not `User` entities?
    pub async fn check_history_status(
        self,
        resolver: &impl Resolver<Entity<T, ST>>,
        certifier_resolver: &impl Resolver<User<Draft>>,
    ) -> Result<Entity<T, Verified>, HistoryVerificationError>
    where
        ST: Clone,
    {
        let mut current = self.clone();

        loop {
            let revision = current.revision();
            // Check current status
            let signed = match current.check_signatures(certifier_resolver).await {
                Err(err) => {
                    let err = HistoryVerificationError::ErrorAtRevision {
                        revision,
                        error: err,
                    };
                    return Err(err);
                },
                Ok(signed) => signed,
            };

            // End at root revision
            if revision == 1 {
                return Ok(self.with_status::<Verified>());
            }

            // Resolve previous revision
            current = match resolver.resolve_revision(&self.urn(), revision - 1).await {
                // Check update between current and previous
                Ok(previous) => match signed.check_update(previous) {
                    // Update verification failed
                    Err(err) => {
                        let err = HistoryVerificationError::UpdateError {
                            revision,
                            error: err,
                        };
                        return Err(err);
                    },
                    // Continue traversing revisions
                    Ok(previous) => previous,
                },
                // Resoltion failed
                Err(err) => {
                    let err = HistoryVerificationError::ErrorAtRevision {
                        revision,
                        error: err,
                    };
                    return Err(err);
                },
            }
        }
    }
}

impl<T, ST> Entity<T, ST>
where
    T: Serialize + DeserializeOwned + Clone + EntityInfoExt,
    ST: Clone,
{
    /// Build an `Entity` from its data (the second step of deserialization)
    /// It guarantees that the `hash` is correct
    pub fn from_data(data: EntityData<T>) -> Result<Entity<T, Draft>, Error> {
        // FIXME[ENTITY]: do we want this? it makes `default` harder to get right...
        if data.name.is_none() {
            return Err(Error::InvalidData("Missing name".to_owned()));
        }
        if data.revision.is_none() {
            return Err(Error::InvalidData("Missing revision".to_owned()));
        }

        if data.revision.unwrap() < 1 {
            return Err(Error::InvalidData("Invalid revision".to_owned()));
        }

        // Check specific invariants
        data.check_invariants()?;

        let mut signatures = HashMap::new();
        if let Some(s) = &data.signatures {
            for (key, sig) in s.iter() {
                let signature = EntitySignature {
                    by: match &sig.user {
                        Some(uri) => Signatory::User(
                            RadUrn::from_str(&uri)
                                .map_err(|_| Error::InvalidUri(uri.to_owned()))?,
                        ),
                        None => Signatory::OwnedKey,
                    },
                    sig: Signature::from_bs58(&sig.sig).ok_or_else(|| {
                        Error::InvalidData(format!("signature data: {}", &sig.sig))
                    })?,
                };
                signatures.insert(key.to_owned(), signature);
            }
        }

        let actual_hash = data.compute_hash()?;
        if let Some(claimed_hash) = &data.hash {
            if claimed_hash != &actual_hash {
                let actual_hash_string = actual_hash.to_string();
                return Err(Error::WrongHash {
                    claimed: claimed_hash.to_string(),
                    actual: actual_hash_string,
                });
            }
        }

        let parent_hash = data.parent_hash.clone();
        let root_hash = match data.root_hash {
            Some(h) => h,
            None => {
                // TODO: error handling for unwrap on revision
                if parent_hash.is_none() && data.revision.unwrap() == 1 {
                    actual_hash.clone()
                } else {
                    return Err(Error::MissingRootHash);
                }
            },
        };

        Ok(Entity::<T, Draft> {
            status_marker: PhantomData,
            name: data.name.unwrap(),
            revision: data.revision.unwrap(),
            rad_version: data.rad_version,
            hash: actual_hash,
            root_hash,
            parent_hash,
            keys: data.keys,
            certifiers: data.certifiers,
            signatures,
            info: data.info,
        })
    }

    /// Helper serialization to JSON writer
    pub fn to_json_writer<W>(&self, writer: W) -> Result<(), Error>
    where
        W: std::io::Write,
    {
        self.to_data().to_json_writer(writer)?;
        Ok(())
    }

    /// Helper serialization to JSON string
    pub fn to_json_string(&self) -> Result<String, Error> {
        self.to_data().to_json_string()
    }

    /// Helper deserialization from JSON reader
    pub fn from_json_reader<R>(r: R) -> Result<Entity<T, Draft>, Error>
    where
        R: std::io::Read,
    {
        Self::from_data(data::EntityData::from_json_reader(r)?)
    }

    /// Helper deserialization from JSON string
    pub fn from_json_str(s: &str) -> Result<Entity<T, Draft>, Error> {
        Self::from_data(data::EntityData::from_json_str(s)?)
    }

    /// Helper deserialization from JSON slice
    pub fn from_json_slice(s: &[u8]) -> Result<Entity<T, Draft>, Error> {
        Self::from_data(data::EntityData::from_json_slice(s)?)
    }
}

pub type GenericEntity<ST> = Entity<EntityInfo, ST>;
pub type GenericDraftEntity = GenericEntity<Draft>;
