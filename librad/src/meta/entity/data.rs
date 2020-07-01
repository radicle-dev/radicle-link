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

use crate::{
    hash::Hash,
    keys::PublicKey,
    meta::{
        entity::{
            CertifierSignature,
            Draft,
            Entity,
            EntityRevision,
            EntityTimestamp,
            Error,
            SelfSignature,
        },
        project::ProjectInfo,
        user::UserInfo,
        RAD_VERSION,
    },
    uri::RadUrn,
};
use olpc_cjson::CanonicalFormatter;
use serde::{de::DeserializeOwned, Deserialize, Serialize, Serializer};
use std::collections::{BTreeMap, HashMap};

/// Helper to serialize entity self signatures in a canonical way
fn serialize_certifiers<S>(
    value: &HashMap<RadUrn, Option<CertifierSignature>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let ordered: BTreeMap<RadUrn, Option<CertifierSignature>> =
        value.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    ordered.serialize(serializer)
}

/// Helper to serialize entity certifier signatures in a canonical way
fn serialize_keys<S>(
    value: &HashMap<PublicKey, Option<SelfSignature>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let ordered: BTreeMap<PublicKey, Option<SelfSignature>> =
        value.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    ordered.serialize(serializer)
}

/// The different kinds of `Entity` types
#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Debug)]
pub enum EntityKind {
    User,
    Project,
}

/// Information payload for all possible `Entity` types
///
/// This is used so we can deserialize `EntityData` without knowing the actual
/// entity type: deserializing as `EntityData<EntityInfo>` the `info` property
/// will contain this payload
///
/// Care has been taken to ensure that the same data can be deserialized both
/// "generically" (as `EntityData<EntityInfo>`) and using its specific type
#[derive(Clone, Deserialize, Serialize, Debug)]
#[serde(untagged)]
pub enum EntityInfo {
    User(UserInfo),
    Project(ProjectInfo),
}

pub trait EntityInfoExt: Sized {
    fn kind(&self) -> EntityKind;
    fn check_invariants<T>(&self, data: &EntityData<T>) -> Result<(), Error>;
    fn as_info(&self) -> EntityInfo;
}

impl EntityInfoExt for EntityInfo {
    fn kind(&self) -> EntityKind {
        match self {
            EntityInfo::User(_) => EntityKind::User,
            EntityInfo::Project(_) => EntityKind::Project,
        }
    }

    fn check_invariants<T>(&self, data: &EntityData<T>) -> Result<(), Error> {
        match self {
            EntityInfo::User(info) => info.check_invariants(data),
            EntityInfo::Project(info) => info.check_invariants(data),
        }
    }

    fn as_info(&self) -> EntityInfo {
        self.clone()
    }
}

/// Raw data for an `Entity`
///
/// This is used in two ways:
///
/// - as an intermediate step in deserialization so that invariants can be
///   enforced more explicitly
/// - as a "builder" when creating new entities (or entity revisions), again so
///   that invariants can be enforced at `build` time when all the data has been
///   collected
#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct EntityData<T> {
    pub rad_version: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<EntityRevision>,
    pub timestamp: EntityTimestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<Hash>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_hash: Option<Hash>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_hash: Option<Hash>,

    #[serde(
        skip_serializing_if = "HashMap::is_empty",
        serialize_with = "serialize_keys",
        default
    )]
    pub keys: HashMap<PublicKey, Option<SelfSignature>>,
    #[serde(
        skip_serializing_if = "HashMap::is_empty",
        serialize_with = "serialize_certifiers",
        default
    )]
    pub certifiers: HashMap<RadUrn, Option<CertifierSignature>>,

    pub info: T,
}

impl<T> EntityData<T>
where
    T: Serialize + DeserializeOwned + Clone + Default + EntityInfoExt,
{
    pub fn new(time: EntityTimestamp) -> Self {
        Self {
            rad_version: RAD_VERSION,
            name: None,
            revision: Some(1),
            timestamp: time,
            hash: None,
            root_hash: None,
            parent_hash: None,
            keys: HashMap::default(),
            certifiers: HashMap::default(),
            info: T::default(),
        }
    }
}

impl<T> EntityData<T>
where
    T: Serialize + DeserializeOwned + Clone + EntityInfoExt,
{
    pub fn to_json_writer<W>(&self, writer: W) -> Result<(), Error>
    where
        W: std::io::Write,
    {
        serde_json::to_writer(writer, self)
            .map_err(|e| Error::SerializationFailed(e.to_string()))?;
        Ok(())
    }
    pub fn to_json_string(&self) -> Result<String, Error> {
        Ok(serde_json::to_string(self).map_err(|e| Error::SerializationFailed(e.to_string()))?)
    }

    pub fn from_json_reader<R>(r: R) -> Result<Self, Error>
    where
        R: std::io::Read,
    {
        serde_json::from_reader(r).map_err(|e| Error::SerializationFailed(e.to_string()))
    }
    pub fn from_json_str(s: &str) -> Result<Self, Error> {
        serde_json::from_str(s).map_err(|e| Error::SerializationFailed(e.to_string()))
    }
    pub fn from_json_slice(s: &[u8]) -> Result<Self, Error> {
        serde_json::from_slice(s).map_err(|e| Error::SerializationFailed(e.to_string()))
    }

    pub fn canonical_data(&self) -> Result<Vec<u8>, Error> {
        let cleaned = Self {
            name: self.name.to_owned(),
            revision: self.revision.to_owned(),
            timestamp: self.timestamp,
            hash: None,
            root_hash: if self.parent_hash.is_some() {
                self.root_hash.to_owned()
            } else {
                None
            },
            parent_hash: self.parent_hash.to_owned(),
            keys: self.keys.iter().map(|(k, _)| (k.clone(), None)).collect(),
            certifiers: self
                .certifiers
                .iter()
                .map(|(k, _)| (k.clone(), None))
                .collect(),
            info: self.info.to_owned(),
            rad_version: self.rad_version,
        };

        let mut buffer: Vec<u8> = vec![];
        let mut ser =
            serde_json::Serializer::with_formatter(&mut buffer, CanonicalFormatter::new());
        cleaned
            .serialize(&mut ser)
            .map_err(|e| Error::SerializationFailed(e.to_string()))?;
        Ok(buffer)
    }

    pub fn compute_hash(&self) -> Result<Hash, Error> {
        Ok(Hash::hash(&self.canonical_data()?))
    }

    pub fn set_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }
    pub fn clear_name(mut self) -> Self {
        self.name = None;
        self
    }
    pub fn set_optional_name(mut self, name: Option<String>) -> Self {
        self.name = name;
        self
    }

    pub fn reset_timestamp(mut self) -> Self {
        self.timestamp = EntityTimestamp::current_time();
        self
    }

    pub fn set_revision(mut self, revision: u64) -> Self {
        self.revision = Some(revision);
        self
    }
    pub fn clear_revision(mut self) -> Self {
        self.revision = None;
        self
    }

    pub fn clear_hash(mut self) -> Self {
        self.hash = None;
        self
    }

    pub fn set_root_hash(mut self, root_hash: Hash) -> Self {
        self.root_hash = Some(root_hash);
        self
    }
    pub fn clear_root_hash(mut self) -> Self {
        self.root_hash = None;
        self
    }

    pub fn set_parent_hash(mut self, parent_hash: Hash) -> Self {
        self.parent_hash = Some(parent_hash);
        self
    }
    pub fn clear_parent_hash(mut self) -> Self {
        self.parent_hash = None;
        self
    }

    pub fn set_parent<ST>(mut self, parent: &Entity<T, ST>) -> Self
    where
        ST: Clone,
    {
        self.parent_hash = Some(parent.hash().clone());
        self.root_hash = Some(parent.root_hash().clone());
        self.revision = Some(parent.revision() + 1);
        self
    }

    pub fn clear_signatures(mut self) -> Self {
        self.keys = self.keys.iter().map(|(k, _)| (k.clone(), None)).collect();
        self.certifiers = self
            .certifiers
            .iter()
            .map(|(k, _)| (k.clone(), None))
            .collect();

        self
    }

    pub fn add_key(mut self, key: PublicKey) -> Self {
        self.keys.insert(key, None);
        self
    }
    pub fn remove_key(mut self, key: &PublicKey) -> Self {
        self.keys.remove(key);
        self
    }
    pub fn clear_keys(mut self) -> Self {
        self.keys.clear();
        self
    }
    pub fn add_keys(mut self, keys: impl IntoIterator<Item = PublicKey>) -> Self {
        for s in keys.into_iter() {
            self.keys.insert(s, None);
        }
        self
    }

    pub fn add_certifier(mut self, certifier: RadUrn) -> Self {
        self.certifiers.insert(certifier, None);
        self
    }
    pub fn remove_certifier(mut self, certifier: &RadUrn) -> Self {
        self.certifiers.remove(certifier);
        self
    }
    pub fn clear_certifiers(mut self) -> Self {
        self.certifiers.clear();
        self
    }
    pub fn add_certifiers(mut self, certifiers: impl IntoIterator<Item = RadUrn>) -> Self {
        for c in certifiers.into_iter() {
            self.certifiers.insert(c, None);
        }
        self
    }

    pub fn map(self, f: impl FnOnce(Self) -> Self) -> Self {
        f(self)
    }

    pub fn check_invariants(&self) -> Result<(), Error> {
        self.info.check_invariants(self)
    }

    pub fn build(self) -> Result<Entity<T, Draft>, Error> {
        Entity::<T, Draft>::from_data(self)
    }
}

pub type GenericEntityData = EntityData<EntityInfo>;
