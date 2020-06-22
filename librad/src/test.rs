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

//! Common testing utilities

#[cfg(test)]
use std::{
    fmt::{Debug, Display},
    io,
    ops::{Deref, DerefMut},
    path::Path,
    str::FromStr,
};

use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};
use tempfile::{tempdir, TempDir};

use crate::{
    meta::entity::{self, data::EntityInfoExt, Entity, Resolver},
    uri::RadUrn,
};

pub(crate) fn json_roundtrip<A>(a: A)
where
    for<'de> A: Debug + PartialEq + serde::Serialize + serde::Deserialize<'de>,
{
    assert_eq!(
        a,
        serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap()
    )
}

pub(crate) fn cbor_roundtrip<A>(a: A)
where
    for<'de> A: Debug + PartialEq + minicbor::Encode + minicbor::Decode<'de>,
{
    assert_eq!(a, minicbor::decode(&minicbor::to_vec(&a).unwrap()).unwrap())
}

pub(crate) fn str_roundtrip<A>(a: A)
where
    A: Debug + PartialEq + Display + FromStr,
    <A as FromStr>::Err: Debug,
{
    assert_eq!(a, a.to_string().parse().unwrap())
}

pub(crate) struct WithTmpDir<A> {
    _tmp: TempDir,
    inner: A,
}

impl<A> WithTmpDir<A> {
    pub(crate) fn new<F, E>(mk_inner: F) -> Result<Self, E>
    where
        F: FnOnce(&Path) -> Result<A, E>,
        E: From<io::Error>,
    {
        let tmp = tempdir()?;
        let inner = mk_inner(tmp.path())?;
        Ok(Self { _tmp: tmp, inner })
    }
}

impl<A> Deref for WithTmpDir<A> {
    type Target = A;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<A> DerefMut for WithTmpDir<A> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

/// A Resolver which always resolves to the same value.
///
/// The `RadUrn` must match, however.
pub(crate) struct ConstResolver<A, S> {
    entity: Entity<A, S>,
    urn: RadUrn,
}

impl<A, S> ConstResolver<A, S>
where
    A: Clone + Default + Serialize + DeserializeOwned + EntityInfoExt,
{
    pub(crate) fn new(entity: Entity<A, S>) -> Self {
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
