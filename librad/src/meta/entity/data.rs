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
    meta::{
        entity::{Entity, EntityStatusUnknown, Error},
        RAD_VERSION,
    },
};
use olpc_cjson::CanonicalFormatter;
use serde::{de::DeserializeOwned, Deserialize, Serialize, Serializer};
use std::collections::{BTreeSet, HashMap, HashSet};

/// Raw data for an entity signature
#[derive(Clone, Deserialize, Serialize, Debug, PartialEq)]
pub struct EntitySignatureData {
    /// `None` for signatures by owned keys, otherwise the certifier URN
    pub user: Option<String>,
    /// The signature data
    pub sig: String,
}

/// Helper to serialize `HashSet` in a canonical way
fn ordered_set<S>(value: &HashSet<String>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let ordered: BTreeSet<String> = value.iter().cloned().collect();
    ordered.serialize(serializer)
}

/// Raw data for an `Entity
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
    pub revision: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_hash: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub signatures: Option<HashMap<String, EntitySignatureData>>,

    #[serde(
        skip_serializing_if = "HashSet::is_empty",
        serialize_with = "ordered_set",
        default
    )]
    pub keys: HashSet<String>,
    #[serde(
        skip_serializing_if = "HashSet::is_empty",
        serialize_with = "ordered_set",
        default
    )]
    pub certifiers: HashSet<String>,

    pub info: T,
}

impl<T> Default for EntityData<T>
where
    T: Serialize + DeserializeOwned + Clone + Default,
{
    fn default() -> Self {
        Self {
            rad_version: RAD_VERSION,
            name: None,
            revision: Some(1),
            hash: None,
            root_hash: None,
            parent_hash: None,
            signatures: None,
            keys: HashSet::default(),
            certifiers: HashSet::default(),
            info: T::default(),
        }
    }
}

impl<T> EntityData<T>
where
    T: Serialize + DeserializeOwned + Clone + Default,
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
        let mut cleaned = EntityData::<T>::default();
        cleaned.name = self.name.to_owned();
        cleaned.revision = self.revision.to_owned();
        cleaned.hash = None;
        cleaned.root_hash = if self.parent_hash.is_some() {
            self.root_hash.to_owned()
        } else {
            None
        };
        cleaned.parent_hash = self.parent_hash.to_owned();
        cleaned.keys = self.keys.to_owned();
        cleaned.certifiers = self.certifiers.to_owned();
        cleaned.info = self.info.to_owned();

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

    pub fn set_root_hash(mut self, root_hash: String) -> Self {
        self.root_hash = Some(root_hash);
        self
    }
    pub fn clear_root_hash(mut self) -> Self {
        self.root_hash = None;
        self
    }

    pub fn set_parent_hash(mut self, parent_hash: String) -> Self {
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
        let parent_hash_text = parent.hash().to_string();
        self.parent_hash = Some(parent_hash_text);
        let root_hash_text = parent.root_hash().to_string();
        self.root_hash = Some(root_hash_text);
        self.revision = Some(parent.revision() + 1);
        self
    }

    pub fn clear_signatures(mut self) -> Self {
        self.signatures = None;
        self
    }

    pub fn add_key(mut self, key: String) -> Self {
        self.keys.insert(key);
        self
    }
    pub fn remove_key(mut self, key: &str) -> Self {
        self.keys.remove(key);
        self
    }
    pub fn clear_keys(mut self) -> Self {
        self.keys.clear();
        self
    }
    pub fn add_keys(mut self, keys: impl IntoIterator<Item = String>) -> Self {
        for s in keys.into_iter() {
            self.keys.insert(s);
        }
        self
    }

    pub fn add_certifier(mut self, certifier: String) -> Self {
        self.certifiers.insert(certifier);
        self
    }
    pub fn remove_certifier(mut self, certifier: &str) -> Self {
        self.certifiers.remove(certifier);
        self
    }
    pub fn clear_certifiers(mut self) -> Self {
        self.certifiers.clear();
        self
    }
    pub fn add_certifiers(mut self, certifiers: impl IntoIterator<Item = String>) -> Self {
        for s in certifiers.into_iter() {
            self.certifiers.insert(s);
        }
        self
    }

    pub fn map(self, f: impl FnOnce(Self) -> Self) -> Self {
        f(self)
    }
}

pub trait EntityBuilder {
    fn check_invariants(&self) -> Result<(), Error>;
}

impl<T> EntityData<T>
where
    T: Serialize + DeserializeOwned + Clone + Default,
    EntityData<T>: EntityBuilder,
{
    pub fn build(self) -> Result<Entity<T, EntityStatusUnknown>, Error> {
        Entity::<T, EntityStatusUnknown>::from_data(self)
    }
}
