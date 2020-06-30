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

#[async_trait]
impl<A, S> Resolver<Entity<A, S>> for ConstResolver<A, S>
where
    A: Clone + Send + Sync + Default + Serialize + DeserializeOwned,
    S: Clone + Send + Sync,
{
    async fn resolve(&self, urn: &RadUrn) -> Result<Entity<A, S>, entity::Error> {
        if &self.urn == urn {
            Ok(self.entity.clone())
        } else {
            Err(entity::Error::ResolutionFailed(urn.clone()))
        }
    }

    async fn resolve_revision(
        &self,
        urn: &RadUrn,
        revision: u64,
    ) -> Result<Entity<A, S>, entity::Error> {
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
