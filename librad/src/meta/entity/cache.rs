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
    meta::entity::{
        Entity,
        EntityCache,
        EntityInfoExt,
        EntityRevision,
        EntityRevisionStatus,
        EntityTimestamp,
        GenericEntity,
        KeyOwnershipStore,
        Verified,
    },
    uri::RadUrn,
};
use serde::{de::DeserializeOwned, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use thiserror::Error;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum Error {
    #[error("Missing entity (urn {0})")]
    MissingEntity(RadUrn),

    #[error("Missing entity revision (entity {0}, revision {1})")]
    MissingEntityRevision(RadUrn, EntityRevision),

    #[error("Missing parent entity (entity {0}, revision {1})")]
    MissingParentEntity(RadUrn, EntityRevision),

    #[error("Missing certifier (entity {0}, revision {1}, certifier {2})")]
    MissingCertifier(RadUrn, EntityRevision, RadUrn),

    #[error("Missing certifier (entity {0}, revision {1}, certifier {2})")]
    MissingSignature(RadUrn, EntityRevision, RadUrn),

    #[error("Entity tainted (entity {0}, revision {1})")]
    EntityTainted(RadUrn, EntityRevision),

    #[error("Signatures revoked")]
    SignaturesRevoked,

    #[error("Missing internal entity id ({0})")]
    MissingInternalEntityId(EntityInternalId),

    #[error("Missing internal revision ({0})")]
    MissingInternalRevision(EntityRevision),

    #[error("Missing key ({0})")]
    MissingKey(PublicKey),
}

type EntityInternalId = usize;
type KeyInternalId = usize;

#[derive(Clone, Debug, PartialEq, Eq)]
struct EntityHashInfo {
    pub id: EntityInternalId,
    pub revision: EntityRevision,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EntitySignatureInfo {
    pub revision: EntityRevision,
    pub time: EntityTimestamp,
    pub key_id: KeyInternalId,
    pub revoked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct EntitySignatureTargetInfo {
    pub id: EntityInternalId,
    pub revision: EntityRevision,
}

#[derive(Clone, Debug)]
struct EntityInfo {
    pub last_verified_revision: Option<GenericEntity<Verified>>,
    pub revisions: Vec<EntityRevisionInfo>,
}

impl EntityInfo {
    pub fn new() -> Self {
        Self {
            last_verified_revision: None,
            revisions: Vec::new(),
        }
    }

    pub fn revision(&self, rev: EntityRevision) -> Result<&EntityRevisionInfo, Error> {
        Ok(self
            .revisions
            .get(rev as usize - 1)
            .ok_or(Error::MissingInternalRevision(rev))?)
    }

    pub fn revision_mut(&mut self, rev: EntityRevision) -> Result<&mut EntityRevisionInfo, Error> {
        Ok(self
            .revisions
            .get_mut(rev as usize - 1)
            .ok_or(Error::MissingInternalRevision(rev))?)
    }

    pub fn revisions_from(
        &self,
        start: EntityRevision,
    ) -> impl IntoIterator<Item = EntityRevision> {
        let end = self.revisions.len() as EntityRevision + 1;
        start..end
    }

    pub fn tainted(&self) -> bool {
        self.revisions
            .last()
            .map_or(false, |rev| rev.status == EntityRevisionStatus::Tainted)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EntityRevisionInfo {
    pub hash: Hash,
    pub status: EntityRevisionStatus,
    pub timestamp: EntityTimestamp,
    pub owned_keys: BTreeSet<KeyInternalId>,
    pub signed_by: BTreeMap<EntityInternalId, EntitySignatureInfo>,
    pub signature_targets: BTreeSet<EntitySignatureTargetInfo>,
}

#[derive(Default)]
pub struct EntityMemoryCache {
    hashes: HashMap<Hash, EntityHashInfo>,
    key_ids: HashMap<PublicKey, KeyInternalId>,
    keys: Vec<PublicKey>,
    entity_ids: HashMap<RadUrn, EntityInternalId>,
    entities: Vec<EntityInfo>,
}

impl EntityMemoryCache {
    fn get_or_create_entity_info<T>(
        &mut self,
        entity: &Entity<T, Verified>,
    ) -> (EntityInternalId, &mut EntityInfo)
    where
        T: Serialize + DeserializeOwned + Clone + EntityInfoExt,
    {
        let urn = entity.urn();
        let id = match self.entity_ids.get(&urn) {
            Some(id) => *id,
            None => {
                let new_id = self.entities.len();
                let info = EntityInfo::new();
                self.entities.push(info);
                self.entity_ids.insert(urn, new_id);
                self.register_hash(entity.hash(), new_id, entity.revision());
                new_id
            },
        };
        (id, &mut self.entities[id])
    }

    fn entity_info(&self, id: EntityInternalId) -> Result<&EntityInfo, Error> {
        Ok(self
            .entities
            .get(id)
            .ok_or(Error::MissingInternalEntityId(id))?)
    }

    fn entity_info_mut(&mut self, id: EntityInternalId) -> Result<&mut EntityInfo, Error> {
        Ok(self
            .entities
            .get_mut(id)
            .ok_or(Error::MissingInternalEntityId(id))?)
    }

    fn register_hash(&mut self, hash: &Hash, id: EntityInternalId, revision: EntityRevision) {
        if !self.hashes.contains_key(hash) {
            self.hashes
                .insert(hash.to_owned(), EntityHashInfo { id, revision });
        }
    }

    fn check_revocations(
        &mut self,
        id: EntityInternalId,
        revision: EntityRevision,
    ) -> Result<bool, Error> {
        if revision <= 1 {
            return Ok(false);
        }

        let info = self.entity_info(id)?;
        let current = info.revision(revision)?;
        let parent = info.revision(revision - 1)?;
        let revoked_signatures: Vec<_> = parent
            .signature_targets
            .iter()
            .filter_map(|target_signature| {
                let signed_revision =
                    self.entity_info(target_signature.id)
                        .ok()
                        .and_then(|target_entity| {
                            target_entity.revision(target_signature.revision).ok()
                        })?;
                let signature_info = signed_revision.signed_by.get(&id)?;

                if signature_info.time >= current.timestamp
                    && !current.owned_keys.contains(&signature_info.key_id)
                {
                    Some(target_signature.to_owned())
                } else {
                    None
                }
            })
            .collect();

        if !revoked_signatures.is_empty() {
            let mut tainted = Vec::new();
            for sig in revoked_signatures {
                tainted.push((sig.id, sig.revision));
                let signed_entity = self.entity_info_mut(sig.id)?;
                let signed_revision = signed_entity.revision_mut(sig.revision)?;
                let revoked_signature = signed_revision
                    .signed_by
                    .get_mut(&id)
                    .ok_or(Error::MissingInternalEntityId(id))?;
                revoked_signature.revoked = true;
            }
            for (id, revision) in tainted {
                self.set_tainted(id, revision)?;
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn apply_signatures<T>(
        &mut self,
        id: EntityInternalId,
        entity: &Entity<T, Verified>,
    ) -> Result<(), Error>
    where
        T: Serialize + DeserializeOwned + Clone + EntityInfoExt,
    {
        let mut signed_by = Vec::new();
        for (certifier_urn, s) in entity.certifiers().iter() {
            let certifier_id = *self.entity_ids.get(&certifier_urn).ok_or_else(|| {
                Error::MissingCertifier(entity.urn(), entity.revision(), certifier_urn.to_owned())
            })?;
            let sig = s.as_ref().ok_or_else(|| {
                Error::MissingSignature(entity.urn(), entity.revision(), certifier_urn.to_owned())
            })?;

            self.entity_info_mut(certifier_id)?
                .revision_mut(sig.revision)
                .map_err(|_| Error::MissingEntityRevision(certifier_urn.to_owned(), sig.revision))?
                .signature_targets
                .insert(EntitySignatureTargetInfo {
                    id,
                    revision: entity.revision(),
                });

            signed_by.push((
                certifier_id,
                EntitySignatureInfo {
                    revision: sig.revision,
                    time: sig.timestamp,
                    key_id: self
                        .key_ids
                        .get(&sig.key)
                        .ok_or_else(|| Error::MissingKey(sig.key.to_owned()))?
                        .to_owned(),
                    revoked: false,
                },
            ));
        }

        let sigs = &mut self
            .entity_info_mut(id)
            .map_err(|_| Error::MissingEntity(entity.urn()))?
            .revision_mut(entity.revision())
            .map_err(|_| Error::MissingEntityRevision(entity.urn(), entity.revision()))?
            .signed_by;
        for (key, sig) in signed_by.iter() {
            sigs.insert(*key, sig.to_owned());
        }

        Ok(())
    }

    fn set_tainted(&mut self, id: EntityInternalId, revision: EntityRevision) -> Result<(), Error> {
        // FIXME: add tainting info
        let mut tainted_revisions = Vec::new();
        let mut tainted_targets = Vec::new();

        for rev in self.entity_info(id)?.revisions_from(revision) {
            tainted_revisions.push((id, rev));
        }

        while !tainted_revisions.is_empty() {
            let (id, r) = tainted_revisions.pop().unwrap();

            let rev = self.entity_info_mut(id)?.revision_mut(r)?;
            if rev.status != EntityRevisionStatus::Tainted {
                rev.status = EntityRevisionStatus::Tainted;
                for sig in rev.signature_targets.iter() {
                    tainted_targets.push(sig.to_owned());
                }
            }

            while !tainted_targets.is_empty() {
                let target = tainted_targets.pop().unwrap();
                for rev in self.entity_info(target.id)?.revisions_from(target.revision) {
                    tainted_revisions.push((id, rev));
                }
            }
        }
        Ok(())
    }

    pub fn register_verified_entity<T>(&mut self, entity: &Entity<T, Verified>) -> Result<(), Error>
    where
        T: Serialize + DeserializeOwned + Clone + EntityInfoExt,
    {
        let urn = entity.urn();
        let revision = entity.revision();
        let mut owned_keys = BTreeSet::new();
        for k in entity.keys().keys() {
            let key_id = match self.key_ids.get(k) {
                Some(key_id) => *key_id,
                None => {
                    let new_key_id = self.keys.len();
                    self.keys.push(k.to_owned());
                    self.key_ids.insert(k.to_owned(), new_key_id);
                    new_key_id
                },
            };
            owned_keys.insert(key_id);
        }
        let (id, info) = self.get_or_create_entity_info(entity);
        let required_previous_revisions = revision as usize - 1;
        if info.revisions.len() < required_previous_revisions {
            return Err(Error::MissingParentEntity(urn, revision));
        };

        let tainted = if info.revisions.len() == required_previous_revisions {
            info.revisions.push(EntityRevisionInfo {
                hash: entity.hash().to_owned(),
                status: EntityRevisionStatus::Verified,
                timestamp: entity.timestamp,
                owned_keys,
                signed_by: BTreeMap::new(),
                signature_targets: BTreeSet::new(),
            });
            info.last_verified_revision = Some(entity.as_generic_entity());
            false
        } else {
            &info.revisions[required_previous_revisions].hash != entity.hash()
        };

        self.apply_signatures(id, entity)?;
        let signatures_revoked = self.check_revocations(id, entity.revision())?;

        if tainted {
            self.set_tainted(id, revision)?;
            Err(Error::EntityTainted(urn, entity.revision()))
        } else if signatures_revoked {
            Err(Error::SignaturesRevoked)
        } else {
            Ok(())
        }
    }

    pub fn is_tainted(&self, uri: &RadUrn) -> bool {
        self.entity_ids
            .get(uri)
            .and_then(|id| self.entity_info(*id).ok())
            .map_or(false, |info| info.tainted())
    }
}

impl EntityCache for EntityMemoryCache {
    fn last_verified_revision(&self, uri: &RadUrn) -> Option<GenericEntity<Verified>> {
        self.entity_ids
            .get(uri)
            .and_then(|id| self.entity_info(*id).ok())
            .and_then(|info| {
                if info.tainted() {
                    None
                } else {
                    info.last_verified_revision.clone()
                }
            })
    }

    fn revision_status(
        &self,
        uri: &RadUrn,
        revision: EntityRevision,
    ) -> Option<EntityRevisionStatus> {
        self.entity_ids
            .get(uri)
            .and_then(|id| self.entity_info(*id).ok())
            .and_then(|info| info.revision(revision).ok())
            .map(|info| info.status.clone())
    }
}

impl KeyOwnershipStore for EntityMemoryCache {
    fn check_ownership(
        &self,
        key: &PublicKey,
        uri: &RadUrn,
        revision: EntityRevision,
        time: EntityTimestamp,
    ) -> bool {
        let key_id = match self.key_ids.get(key) {
            Some(key_id) => key_id,
            None => return false,
        };
        let id = match self.entity_ids.get(uri) {
            Some(id) => id,
            None => return false,
        };
        let info = match self.entity_info(*id).ok() {
            Some(info) => info,
            None => return false,
        };
        let rev = match info.revision(revision).ok() {
            Some(rev) => rev,
            None => return false,
        };

        if !rev.owned_keys.contains(key_id) {
            return false;
        }
        match info.revision(revision + 1).ok() {
            Some(next) => {
                if next.owned_keys.contains(key_id) {
                    true
                } else {
                    next.timestamp > time
                }
            },
            None => true,
        }
    }
}
