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

pub mod data;

use crate::{
    id::{uri::RadicleUri, user::User},
    keys::device::{Key, PublicKey, Signature},
};
use async_trait::async_trait;
use futures::stream::{Stream, StreamExt};
use multihash::{Multihash, Sha2_256};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    collections::{HashMap, HashSet},
    iter::FromIterator,
    str::FromStr,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Serialization failed ({0})")]
    SerializationFailed(serde_json::error::Error),

    #[error("Invalid UTF8 ({0})")]
    InvalidUtf8(std::string::FromUtf8Error),

    #[error("Invalid buffer encoding ({0})")]
    InvalidBufferEncoding(String),

    #[error("Invalid hash ({0})")]
    InvalidHash(String),

    #[error("Invalid URI ({0})")]
    InvalidUri(String),

    #[error("Signature already present ({0})")]
    SignatureAlreadyPresent(PublicKey),

    #[error("Invalid data ({0})")]
    InvalidData(String),

    #[error("Key not present ({0})")]
    KeyNotPresent(PublicKey),

    #[error("User not present ({0})")]
    UserNotPresent(RadicleUri),

    #[error("User key not present (uri {0}, key {1})")]
    UserKeyNotPresent(RadicleUri, PublicKey),

    #[error("Signature missing")]
    SignatureMissing,

    #[error("Signature decoding failed")]
    SignatureDecodingFailed,

    #[error("Signature verification failed")]
    SignatureVerificationFailed,

    #[error("Resolution failed (uri {0})")]
    ResolutionFailed(String),
}

#[derive(Debug, Error)]
pub enum UpdateVerificationError {
    #[error("Non monotonic revision")]
    NonMonotonicRevision,

    #[error("Wrong parent hash")]
    WrongParentHash,

    #[error("Update without previous quorum")]
    NoPreviousQuorum,

    #[error("Update without current quorum")]
    NoCurrentQuorum,
}

#[derive(Debug, Error)]
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

/// A type expressing *who* is signing an `Entity`:
///
///  * either a specific user (identified by their URN),
///  * or the entity itself (with an owned key).
#[derive(Clone, Debug)]
pub enum Signatory {
    User(RadicleUri),
    OwnedKey,
}

/// A signature for an `Entity`
#[derive(Clone, Debug)]
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
    async fn resolve(&self, uri: &RadicleUri) -> Result<T, Error>;
}

/// The base entity definition.
///
/// Entities have the following properties:
///
/// - They can evolve over time, so they have a sequence of revisions.
/// - Their identity is stable (it does not change over time), and it is the
///   hash of their initial revision.
/// - Each revision contains the hash of the previous revision, which is also
///   hashed, so that the sequence of revisions is a Merkel tree (actually just
///   a list).
/// - They can be signed, either with a key they own, or using a key belonging
///   to a different entity (the certifier); note that when applying multiple
///   signatures, signatures are not themselves signed (what is signed is always
///   only the entity itself).
/// - Each revision specifies the set of owned keys and trusted certifiers.
/// - Each revision must be signed by all its owned keys and trusted certifiers.
/// - Each subsequent revision must be signed by a quorum of the previous keys
///   and certifiers, to prove that the entity evolution is actually under the
///   control of its current "owners" (the idea is taken from TUF).
#[derive(Clone)]
pub struct Entity<T> {
    /// The entity name (useful for humans because the hash is unreadable)
    name: String,
    /// Entity revision, to be incremented at each entity update
    revision: u64,
    /// Entity hash, computed on everything except the signatures and
    /// (obviously) the hash itself
    hash: Multihash,
    /// Hash of the previous revision, `None` for the initial revision
    /// (in this case the entity hash is actually the entity ID)
    parent_hash: Option<Multihash>,
    /// Set of signatures
    signatures: HashMap<PublicKey, EntitySignature>,
    /// Set of owned keys
    keys: HashSet<PublicKey>,
    /// Set of certifiers (entities identified by their URN)
    certifiers: HashSet<RadicleUri>,
    /// Specific `Entity` data
    info: T,
}

impl<T> Entity<T>
where
    T: Serialize + DeserializeOwned + Clone + Default,
{
    /// `name` getter
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `revision` getter
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Build and `Entity` from its data (the second step of deserialization)
    /// It guarantees that the `hash` is correct
    pub fn from_data(data: data::EntityData<T>) -> Result<Self, Error> {
        if data.name.is_none() {
            return Err(Error::InvalidData("Missing name".to_owned()));
        }
        if data.revision.is_none() {
            return Err(Error::InvalidData("Missing revision".to_owned()));
        }
        if data.keys.is_empty() {
            return Err(Error::InvalidData("Missing keys".to_owned()));
        }

        let mut keys = HashSet::new();
        for k in data.keys.iter() {
            keys.insert(
                PublicKey::from_bs58(k).ok_or_else(|| Error::InvalidData(format!("key: {}", k)))?,
            );
        }

        let mut certifiers = HashSet::new();
        for c in data.certifiers.iter() {
            certifiers.insert(
                RadicleUri::from_str(c)
                    .map_err(|_| Error::InvalidData(format!("certifier: {}", c)))?,
            );
        }

        let mut signatures = HashMap::new();
        if let Some(s) = &data.signatures {
            for (k, sig) in s.iter() {
                let key = PublicKey::from_bs58(k)
                    .ok_or_else(|| Error::InvalidData(format!("signature key: {}", k)))?;
                let signature = EntitySignature {
                    by: match &sig.user {
                        Some(uri) => Signatory::User(RadicleUri::from_str(&uri)?),
                        None => Signatory::OwnedKey,
                    },
                    sig: Signature::from_bs58(&sig.sig).ok_or_else(|| {
                        Error::InvalidData(format!("signature data: {}", &sig.sig))
                    })?,
                };
                signatures.insert(key, signature);
            }
        }

        let actual_hash = data.compute_hash()?;
        if let Some(s) = &data.hash {
            let claimed_hash = {
                let bytes = bs58::decode(s.as_bytes())
                    .with_alphabet(bs58::alphabet::BITCOIN)
                    .into_vec()
                    .map_err(|_| Error::InvalidBufferEncoding(s.to_owned()))?;
                Multihash::from_bytes(bytes).map_err(|_| Error::InvalidHash(s.to_owned()))?
            };
            if claimed_hash != actual_hash {
                return Err(Error::InvalidHash(s.to_owned()));
            }
        }

        let parent_hash = match data.parent_hash {
            Some(s) => {
                let bytes = bs58::decode(s.as_bytes())
                    .with_alphabet(bs58::alphabet::BITCOIN)
                    .into_vec()
                    .map_err(|_| Error::InvalidBufferEncoding(s.to_owned()))?;
                let hash =
                    Multihash::from_bytes(bytes).map_err(|_| Error::InvalidHash(s.to_owned()))?;
                Some(hash)
            },
            None => None,
        };

        Ok(Self {
            name: data.name.unwrap(),
            revision: data.revision.unwrap().to_owned(),
            hash: actual_hash,
            parent_hash,
            keys,
            certifiers,
            signatures,
            info: data.info,
        })
    }

    /// Turn the entity in to its raw data
    /// (first step of serialization and reverse of `from_data`)
    pub fn to_data(&self) -> data::EntityData<T> {
        let mut signatures = HashMap::new();
        for (k, s) in self.signatures() {
            signatures.insert(
                k.to_bs58(),
                data::EntitySignatureData {
                    user: match &s.by {
                        Signatory::User(uri) => Some(uri.to_string()),
                        Signatory::OwnedKey => None,
                    },
                    sig: s.sig.to_bs58(),
                },
            );
        }

        let keys = HashSet::from_iter(self.keys().iter().map(|k| k.to_bs58()));
        let certifiers = HashSet::from_iter(self.certifiers().iter().map(|c| c.to_string()));

        data::EntityData {
            name: Some(self.name.to_owned()),
            revision: Some(self.revision),
            hash: Some(
                bs58::encode(&self.hash)
                    .with_alphabet(bs58::alphabet::BITCOIN)
                    .into_string(),
            ),
            parent_hash: self.parent_hash.to_owned().map(|h| {
                bs58::encode(h)
                    .with_alphabet(bs58::alphabet::BITCOIN)
                    .into_string()
            }),
            signatures: Some(signatures),
            keys,
            certifiers,
            info: self.info.to_owned(),
        }
    }

    /// Helper to build a new entity cloning the current one
    /// (signatures are cleared because they would be invalid anyway)
    pub fn to_builder(&self) -> data::EntityData<T> {
        self.to_data().clear_hash().clear_signatures()
    }

    /// `hash` getter
    pub fn hash(&self) -> &Multihash {
        &self.hash
    }

    /// `uri` getter
    pub fn uri(&self) -> RadicleUri {
        RadicleUri::new(self.hash.to_owned())
    }

    /// `parent_hash` getter
    pub fn parent_hash(&self) -> &Option<Multihash> {
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
    pub fn certifiers(&self) -> &HashSet<RadicleUri> {
        &self.certifiers
    }
    /// Certifiers count
    fn certifiers_count(&self) -> usize {
        self.certifiers.len()
    }
    /// Check certifier presence
    fn has_certifier(&self, c: &RadicleUri) -> bool {
        self.certifiers.contains(c)
    }

    /// Turn the entity into its canonical data representation
    /// (for hashing or signing)
    pub fn canonical_data(&self) -> Result<Vec<u8>, Error> {
        self.to_data().canonical_data()
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
    pub fn from_json_reader<R>(r: R) -> Result<Self, Error>
    where
        R: std::io::Read,
    {
        Self::from_data(data::EntityData::from_json_reader(r)?)
    }

    /// Helper deserialization from JSON strng
    pub fn from_json_str(s: &str) -> Result<Self, Error> {
        Self::from_data(data::EntityData::from_json_str(s)?)
    }

    /// Compute the entity hash (for validation)
    /// FIXME: this is useless and should be removed: the hash is checked and
    /// eventually computed in `from_data` and cannot be changed after that
    pub fn compute_hash(&self) -> Result<Multihash, Error> {
        Ok(Sha2_256::digest(&self.canonical_data()?))
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
        resolver: &impl Resolver<User>,
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
    /// FIXME: we should check the hash instead: it is cheaper and makes also
    /// verification way faster because we would not need to rebuild the
    /// canonical data at every check (we can trust the hash correctness)
    pub fn compute_signature(&self, key: &Key) -> Result<Signature, Error> {
        Ok(key.sign(&self.canonical_data()?))
    }

    /// Given a private key, sign the current entity
    ///
    /// The following checks are performed:
    ///
    /// - the entity has not been already signed using this same key
    /// - this key is allowed to sign the entity (using `check_key`)
    pub async fn sign(
        &mut self,
        key: &Key,
        by: &Signatory,
        resolver: &impl Resolver<User>,
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
        resolver: &impl Resolver<User>,
    ) -> Result<(), Error> {
        self.check_key(key, by, resolver).await?;
        if signature.verify(&self.canonical_data()?, key) {
            Ok(())
        } else {
            Err(Error::SignatureVerificationFailed)
        }
    }

    /// Check the validity of this entty (only this revision is checked)
    ///
    /// This checks that:
    /// - every owned key and certifier has a corresponding signature
    /// - only owned keys and certifiers have signed the entity
    pub async fn check_validity(&self, resolver: &impl Resolver<User>) -> Result<(), Error> {
        let mut keys = HashSet::<PublicKey>::from_iter(self.keys().iter().cloned());
        let mut users = HashSet::<RadicleUri>::from_iter(self.certifiers().iter().cloned());

        for (k, s) in self.signatures() {
            self.check_signature(k, &s.by, &s.sig, resolver).await?;
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
            Ok(())
        } else {
            Err(Error::SignatureMissing)
        }
    }

    /// Convenience method to check if an entity is valid
    pub async fn is_valid(&self, resolver: &impl Resolver<User>) -> bool {
        self.check_validity(resolver).await.is_ok()
    }

    /// Given an entity and its previous revision check that the update is
    /// valid:
    ///
    /// - the revision has been incremented
    /// - the parent hash is correct
    /// - the TUF quorum rules have been observed
    ///
    /// FIXME: only allow exact `+1`increments so that the revision history has
    /// no holes
    /// FIXME: probably we should merge owned keys and certifiers when checking
    /// the quorum rules (now we are handling them separately)
    pub fn check_update(&self, previous: &Self) -> Result<(), UpdateVerificationError> {
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

        Ok(())
    }

    /// Checks that the whole revision history of an entity is valid
    ///
    /// FIXME: also check that the first revision hash matches the entity URN
    pub async fn check_history<R>(
        resolver: &impl Resolver<User>,
        revisions: R,
    ) -> Result<(), HistoryVerificationError>
    where
        R: Stream<Item = Entity<T>> + Unpin,
    {
        let mut revisions = revisions;
        let current = revisions.next().await;
        let mut current = match current {
            None => {
                return Err(HistoryVerificationError::EmptyHistory);
            },
            Some(entity) => entity,
        };

        let revision = current.revision();
        current
            .check_validity(resolver)
            .await
            .map_err(|error| HistoryVerificationError::ErrorAtRevision { revision, error })?;

        while let Some(previous) = revisions.next().await {
            let revision = current.revision();
            previous
                .check_validity(resolver)
                .await
                .map_err(|error| HistoryVerificationError::ErrorAtRevision { revision, error })?;
            current
                .check_update(&previous)
                .map_err(|error| HistoryVerificationError::UpdateError { revision, error })?;
            current = previous;
        }

        Ok(())
    }
}
