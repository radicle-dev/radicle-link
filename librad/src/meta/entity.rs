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
    collections::HashMap,
    convert::{Into, TryFrom},
    marker::PhantomData,
    time::{Duration, SystemTime, UNIX_EPOCH},
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

pub mod cache;
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

    #[error("Certifier not present (uri {0})")]
    CertifierNotPresent(RadUrn),

    #[error("Signature missing")]
    SignatureMissing,

    #[error("Signature decoding failed")]
    SignatureDecodingFailed,

    #[error("Signature verification failed (key {0})")]
    SignatureVerificationFailed(PublicKey),

    #[error("Signature ownership check failed (key {0})")]
    SignatureOwnershipCheckFailed(PublicKey),

    #[error("Resolution failed ({0})")]
    ResolutionFailed(RadUrn),

    #[error("Resolution at revision failed ({0}, revision {1})")]
    RevisionResolutionFailed(RadUrn, u64),

    #[error("Entity cache error ({0})")]
    CacheError(#[from] cache::Error),
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
    ErrorAtRevision {
        revision: EntityRevision,
        error: Error,
    },

    #[error("Update error (rev {revision:?}, err {error:?})")]
    UpdateError {
        revision: EntityRevision,
        error: UpdateVerificationError,
    },

    #[error("Entity cache error ({0})")]
    CacheError(#[from] cache::Error),
}

/// Type representing an entity revision
pub type EntityRevision = u64;

/// Timestamp for entities and signatures, as milliseconds from Unix epoch
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct EntityTimestamp(i64);

impl EntityTimestamp {
    /// Current time as Entity timestamp
    pub fn current_time() -> Self {
        EntityTimestamp(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_millis() as i64),
        )
    }

    /// Milliseconds from Unix epoch
    pub fn epoch_millis(self) -> i64 {
        self.0
    }

    /// Time elpsed since another timestamp, if positive or zero
    pub fn time_since(self, other: Self) -> Option<Duration> {
        if self.0 >= other.0 {
            Some(Duration::from_millis((self.0 - other.0) as u64))
        } else {
            None
        }
    }

    /// Time interval after another timestamp, if positive or zero
    pub fn time_after(self, other: Self) -> Option<Duration> {
        other.time_since(self)
    }

    /// Check that a timestamp is included in a time interval
    /// (before closed, after open)
    pub fn is_between(self, before: Self, after: Self) -> bool {
        self >= before && self < after
    }
}

impl Into<SystemTime> for EntityTimestamp {
    fn into(self) -> SystemTime {
        if self.0 >= 0 {
            UNIX_EPOCH
                .checked_add(Duration::from_millis(self.0 as u64))
                .unwrap()
        } else {
            UNIX_EPOCH
                .checked_sub(Duration::from_millis((-self.0) as u64))
                .unwrap()
        }
    }
}

/// Type witness for a fully verified [`Entity`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified;

/// Type witness for a signed [`Entity`] whose signature keys ownership has been
/// checked (they actually belonged to the specified entities at the appropriate
/// time and revision).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignaturesChecked;

/// Type witness for a signed [`Entity`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signed;

/// Type witness for a draft [`Entity`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft;

/// Verification status of an entity revision
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntityRevisionStatus {
    /// Unverified, signatures are still missing or otherwise invalid
    Draft,
    /// Fully verified
    Verified,
    /// Fully verified but tainted because of a retroactive key revocation
    /// or a legitimate fork of the same revision
    /// TODO: add relevant info to this status
    Tainted,
}

/// A type expressing *who* is signing an `Entity`
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Signatory {
    /// A specific certifier (identified by their URN)
    Certifier(RadUrn, u64),
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

fn build_signature_data(
    hash: &Hash,
    revision: EntityRevision,
    timestamp: EntityTimestamp,
) -> Vec<u8> {
    let mut data: Vec<u8> = Vec::with_capacity(hash.as_bytes().len() + 8 + 8);
    data.extend_from_slice(&revision.to_ne_bytes());
    data.extend_from_slice(&timestamp.epoch_millis().to_ne_bytes());
    data.extend_from_slice(hash.as_bytes());
    data
}

fn build_signature(
    key: &SecretKey,
    hash: &Hash,
    revision: EntityRevision,
    timestamp: EntityTimestamp,
) -> Signature {
    let data = build_signature_data(hash, revision, timestamp);
    key.sign(&data)
}

fn verify_signature(
    sig: &Signature,
    key: &PublicKey,
    hash: &Hash,
    revision: EntityRevision,
    timestamp: EntityTimestamp,
) -> bool {
    let data = build_signature_data(hash, revision, timestamp);
    sig.verify(&data, key)
}

/// A signature for an `Entity`, when signed using an owned key
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct EntitySelfSignature {
    /// Signature time
    pub timestamp: EntityTimestamp,
    /// The signature data
    pub sig: Signature,
}

impl EntitySelfSignature {
    fn build(
        key: &SecretKey,
        hash: &Hash,
        revision: EntityRevision,
        timestamp: EntityTimestamp,
    ) -> Self {
        let sig = build_signature(key, hash, revision, timestamp);
        Self { timestamp, sig }
    }

    fn new(key: &SecretKey, hash: &Hash, revision: EntityRevision) -> Self {
        Self::build(key, hash, revision, EntityTimestamp::current_time())
    }

    fn verify(&self, key: &PublicKey, hash: &Hash, revision: EntityRevision) -> bool {
        verify_signature(&self.sig, key, hash, revision, self.timestamp)
    }
}

/// A signature for an `Entity`, when signed by a certifier
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct EntityCertifierSignature {
    /// Signature time
    pub timestamp: EntityTimestamp,
    /// Certifier revision
    pub revision: EntityRevision,
    /// Key used by the certifier
    pub key: PublicKey,
    /// The signature data
    pub sig: Signature,
}

impl EntityCertifierSignature {
    fn build(
        key: &SecretKey,
        hash: &Hash,
        revision: EntityRevision,
        timestamp: EntityTimestamp,
    ) -> Self {
        let sig = build_signature(key, hash, revision, timestamp);
        Self {
            timestamp,
            revision,
            key: key.public(),
            sig,
        }
    }

    fn new(key: &SecretKey, hash: &Hash, revision: EntityRevision) -> Self {
        Self::build(key, hash, revision, EntityTimestamp::current_time())
    }

    fn verify(&self, hash: &Hash) -> bool {
        verify_signature(&self.sig, &self.key, hash, self.revision, self.timestamp)
    }
}

/// An URN resolver that turns URNs into `Entity` instances
/// (`T` is the entity type)
#[async_trait]
pub trait Resolver<T> {
    /// Resolve the given URN and deserialize the target `Entity`
    async fn resolve(&self, uri: &RadUrn) -> Result<T, Error>;
    async fn resolve_revision(&self, uri: &RadUrn, revision: EntityRevision) -> Result<T, Error>;
}

/// Stores (or caches) information about whether an entity owns (or owned) a key
pub trait EntityKeyOwnershipStore {
    /// Checks if `key` is or was owned by the entity identified by `uri` at the
    /// given `revision` and `time`
    fn check_ownership(
        &self,
        key: &PublicKey,
        uri: &RadUrn,
        revision: EntityRevision,
        time: EntityTimestamp,
    ) -> bool;
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
    revision: EntityRevision,
    /// Entity revision creation timestamp
    timestamp: EntityTimestamp,
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
    /// Set of owned keys and signatures
    keys: HashMap<PublicKey, Option<EntitySelfSignature>>,
    /// Set of certifiers (entities identified by their URN) and signatures
    certifiers: HashMap<RadUrn, Option<EntityCertifierSignature>>,
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
    ST: Clone,
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

    pub fn as_generic_entity(&self) -> GenericEntity<ST> {
        GenericEntity {
            status_marker: PhantomData,
            name: self.name.clone(),
            revision: self.revision,
            timestamp: self.timestamp,
            rad_version: self.rad_version,
            hash: self.hash.clone(),
            root_hash: self.root_hash.clone(),
            parent_hash: self.parent_hash.clone(),
            keys: self.keys.clone(),
            certifiers: self.certifiers.clone(),
            info: self.info.as_info(),
        }
    }

    /// Turn the entity in to its raw data
    /// (first step of serialization and reverse of [`Entity::from_data`])
    pub fn to_data(&self) -> EntityData<T> {
        EntityData {
            name: Some(self.name.to_owned()),
            revision: Some(self.revision),
            timestamp: self.timestamp,
            rad_version: self.rad_version,
            hash: Some(self.hash.clone()),
            root_hash: Some(self.root_hash.clone()),
            parent_hash: self.parent_hash.clone(),
            keys: self.keys.clone(),
            certifiers: self.certifiers.clone(),
            info: self.info.to_owned(),
        }
    }

    /// Helper to build a new entity cloning the current one
    /// (signatures are cleared because they would be invalid anyway)
    pub fn to_builder(&self) -> EntityData<T> {
        self.to_data()
            .clear_hash()
            .clear_signatures()
            .reset_timestamp()
    }

    /// Helper to build a new entity cloning the current one
    /// (signatures are cleared because they would be invalid anyway)
    pub fn prepare_next_revision(&self) -> EntityData<T> {
        self.to_builder().set_parent(self)
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
        RadUrn::new(self.root_hash.to_owned(), Protocol::Git, Path::new())
    }

    /// `parent_hash` getter
    pub fn parent_hash(&self) -> &Option<Hash> {
        &self.parent_hash
    }

    /// `keys` getter
    pub fn keys(&self) -> &HashMap<PublicKey, Option<EntitySelfSignature>> {
        &self.keys
    }
    /// Keys count
    fn keys_count(&self) -> usize {
        self.keys.len()
    }
    /// Check key presence
    fn has_key(&self, key: &PublicKey) -> bool {
        self.keys.contains_key(key)
    }

    /// `certifiers` getter
    pub fn certifiers(&self) -> &HashMap<RadUrn, Option<EntityCertifierSignature>> {
        &self.certifiers
    }
    /// Certifiers count
    fn certifiers_count(&self) -> usize {
        self.certifiers.len()
    }
    /// Check certifier presence
    fn has_certifier(&self, c: &RadUrn) -> bool {
        self.certifiers.contains_key(c)
    }

    /// Turn the entity into its canonical data representation
    /// (for hashing)
    pub fn canonical_data(&self) -> Result<Vec<u8>, Error> {
        self.to_data().canonical_data()
    }

    /// Given a private key owned by the entity, sign the current entity
    ///
    /// The following checks are performed:
    ///
    /// - the entity has not been already signed using this same key
    /// - this key is owned by the current entity
    pub fn sign_owned(&mut self, key: &SecretKey) -> Result<(), Error> {
        let hash = self.hash().clone();
        let revision = self.revision;
        let public_key = key.public();
        self.keys
            .get_mut(&public_key)
            .ok_or_else(|| Error::KeyNotPresent(public_key.clone()))
            .map(|opt| match opt {
                Some(_) => Err(Error::SignatureAlreadyPresent(public_key)),
                None => {
                    *opt = Some(EntitySelfSignature::new(key, &hash, revision));
                    Ok(())
                },
            })??;
        Ok(())
    }

    /// Given a private key owned by a verified user, sign the current entity
    ///
    /// The following checks are performed:
    ///
    /// - the entity has not been already signed using this same key
    /// - this key is owned by the provided user
    pub fn sign_by_user(&mut self, key: &SecretKey, user: &User<Verified>) -> Result<(), Error> {
        let hash = self.hash().clone();
        let revision = self.revision;
        let urn = user.urn();
        let public_key = key.public();
        if !user.has_key(&public_key) {
            return Err(Error::UserKeyNotPresent(user.urn(), public_key));
        }
        self.certifiers
            .get_mut(&urn)
            .ok_or_else(|| Error::CertifierNotPresent(urn.to_owned()))
            .map(|opt| match opt {
                Some(_) => Err(Error::SignatureAlreadyPresent(public_key)),
                None => {
                    *opt = Some(EntityCertifierSignature::new(key, &hash, revision));
                    Ok(())
                },
            })??;
        Ok(())
    }

    fn with_status<NewSt>(self) -> Entity<T, NewSt> {
        Entity::<T, NewSt> {
            status_marker: PhantomData,
            name: self.name,
            revision: self.revision,
            timestamp: self.timestamp,
            rad_version: self.rad_version,
            hash: self.hash,
            root_hash: self.root_hash,
            parent_hash: self.parent_hash,
            keys: self.keys,
            certifiers: self.certifiers,
            info: self.info,
        }
    }

    //FIXME[MASSI] this is only for `#[cfg(test)]`!!!
    //#[cfg(test)]
    pub fn as_verified(&self) -> Entity<T, Verified> {
        Entity::<T, Verified> {
            status_marker: PhantomData,
            name: self.name.clone(),
            revision: self.revision,
            timestamp: self.timestamp,
            rad_version: self.rad_version,
            hash: self.hash.clone(),
            root_hash: self.root_hash.clone(),
            parent_hash: self.parent_hash.clone(),
            keys: self.keys.clone(),
            certifiers: self.certifiers.clone(),
            info: self.info.clone(),
        }
    }
}

impl<T> Entity<T, Draft>
where
    T: Serialize + DeserializeOwned + Clone + EntityInfoExt,
{
    /// Compute the signature status of this entity
    /// (only this revision is checked)
    ///
    /// This checks that:
    /// - every owned key and certifier has a corresponding signature
    /// - the first revision has no parent and a matching root hash
    pub fn check_signatures(self) -> Result<Entity<T, Signed>, Error> {
        if self.revision == 1 && (self.parent_hash.is_some() || self.root_hash != self.hash) {
            // TODO: define a better error if `self.parent_hash.is_some()`
            // (should be "revision 1 cannot have a parent hash")
            return Err(Error::InvalidRootHash);
        }

        for (k, s) in self.keys.iter() {
            match s {
                Some(sig) => {
                    if !sig.verify(k, self.hash(), self.revision) {
                        return Err(Error::SignatureVerificationFailed(k.to_owned()));
                    }
                },
                None => return Err(Error::SignatureMissing),
            }
        }

        for (_, s) in self.certifiers.iter() {
            match s {
                Some(sig) => {
                    if !sig.verify(self.hash()) {
                        return Err(Error::SignatureVerificationFailed(sig.key.to_owned()));
                    }
                },
                None => return Err(Error::SignatureMissing),
            }
        }

        Ok(self.with_status::<Signed>())
    }
}

impl<T> Entity<T, Signed>
where
    T: Serialize + DeserializeOwned + Clone + EntityInfoExt,
{
    /// Compute the signature status of this entity
    /// (only this revision is checked)
    ///
    /// This checks that:
    /// - every owned key and certifier has a corresponding signature
    /// - the first revision has no parent and a matching root hash
    pub fn check_signatures_ownership(
        self,
        store: &impl EntityKeyOwnershipStore,
    ) -> Result<Entity<T, SignaturesChecked>, Error> {
        for (urn, s) in self.certifiers.iter() {
            match s {
                Some(s) => {
                    if !store.check_ownership(&s.key, urn, s.revision, s.timestamp) {
                        return Err(Error::SignatureOwnershipCheckFailed(s.key.to_owned()));
                    }
                },
                None => return Err(Error::SignatureMissing),
            }
        }

        Ok(self.with_status::<SignaturesChecked>())
    }
}

impl<T> Entity<T, SignaturesChecked>
where
    T: Serialize + DeserializeOwned + Clone + EntityInfoExt,
{
    /// Given an entity and its previous revision check that the update is
    /// valid:
    ///
    /// - the revision has been incremented
    /// - the parent hash is correct
    /// - the root hash is correct
    /// - the TUF quorum rules have been observed
    ///
    /// FIXME[ENTITY]: probably we should merge owned keys and certifiers when
    /// checking the quorum rules (now we are handling them separately)
    pub fn check_update(
        self,
        previous: &Option<Entity<T, Verified>>,
    ) -> Result<Entity<T, Verified>, UpdateVerificationError> {
        let previous = match previous {
            None => {
                if self.revision != 1 || self.parent_hash.is_some() {
                    return Err(UpdateVerificationError::WrongParentHash);
                } else if self.root_hash() != self.hash() {
                    return Err(UpdateVerificationError::WrongRootHash);
                } else {
                    return Ok(self.with_status::<Verified>());
                }
            },
            Some(p) => p,
        };

        if self.revision != previous.revision + 1 {
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

        let retained_keys = self
            .keys()
            .iter()
            .filter(|(k, _)| previous.has_key(k))
            .count();
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
            .filter(|(c, _)| previous.has_certifier(c))
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

        Ok(self.with_status::<Verified>())
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
            timestamp: data.timestamp,
            rad_version: data.rad_version,
            hash: actual_hash,
            root_hash,
            parent_hash,
            keys: data.keys,
            certifiers: data.certifiers,
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
