// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use serde::{de::DeserializeOwned, Serialize};

use librad::{
    meta::entity::{self, data::EntityInfoExt, Entity, Resolver},
    uri::RadUrn,
};

/// A Resolver which always resolves to the same value.
///
/// The `RadUrn` must match, however.
pub struct ConstResolver<A, S> {
    entity: Entity<A, S>,
    urn: RadUrn,
}

impl<A, S> ConstResolver<A, S>
where
    A: Clone + Default + Serialize + DeserializeOwned + EntityInfoExt,
{
    pub fn new(entity: Entity<A, S>) -> Self {
        let urn = entity.urn();
        Self { entity, urn }
    }
}

impl<A, S> Resolver<Entity<A, S>> for ConstResolver<A, S>
where
    A: Clone + Send + Sync + Default + Serialize + DeserializeOwned,
    S: Clone + Send + Sync,
{
    fn resolve(&self, urn: &RadUrn) -> Result<Entity<A, S>, entity::Error> {
        if &self.urn == urn {
            Ok(self.entity.clone())
        } else {
            Err(entity::Error::ResolutionFailed(urn.clone()))
        }
    }

    fn resolve_revision(&self, urn: &RadUrn, revision: u64) -> Result<Entity<A, S>, entity::Error> {
        if &self.urn == urn {
            Ok(self.entity.clone())
        } else {
            Err(entity::Error::RevisionResolutionFailed(
                urn.clone(),
                revision,
            ))
        }
    }
}
