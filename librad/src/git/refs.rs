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
    collections::{BTreeMap, HashMap, HashSet},
    fmt::{self, Debug},
    hash::Hash,
    iter,
    ops::{Deref, DerefMut},
};

use serde::{de::Visitor, Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

use crate::{
    internal::canonical::{Cjson, CjsonError},
    keys::{SecretKey, Signature},
    peer::PeerId,
};

#[derive(Debug)]
pub struct Oid(pub git2::Oid);

impl Serialize for Oid {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.to_string().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Oid {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct OidVisitor;

        impl<'de> Visitor<'de> for OidVisitor {
            type Value = Oid;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a git2::Oid")
            }

            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                s.parse().map(Oid).map_err(serde::de::Error::custom)
            }
        }

        deserializer.deserialize_str(OidVisitor)
    }
}

impl Deref for Oid {
    type Target = git2::Oid;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// The transitive tracking graph, up to 3 degrees
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Remotes<A: PartialEq + Eq + Hash>(HashMap<A, HashMap<A, HashSet<A>>>);

impl<A> Remotes<A>
where
    A: PartialEq + Eq + Hash,
{
    pub fn cutoff(self) -> HashMap<A, HashSet<A>>
    where
        A: Clone,
    {
        self.0
            .into_iter()
            .map(|(k, v)| (k, v.keys().cloned().collect()))
            .collect()
    }

    pub fn flatten(&self) -> impl Iterator<Item = &A> {
        self.0.iter().flat_map(|(k, v)| {
            iter::once(k).chain(
                v.iter()
                    .flat_map(|(k1, v1)| iter::once(k1).chain(v1.iter())),
            )
        })
    }

    pub fn from_map(map: HashMap<A, HashMap<A, HashSet<A>>>) -> Self {
        Self(map)
    }

    pub fn boxed(self) -> Box<Self> {
        Box::new(self)
    }
}

impl<A> Deref for Remotes<A>
where
    A: PartialEq + Eq + Hash,
{
    type Target = HashMap<A, HashMap<A, HashSet<A>>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<A> DerefMut for Remotes<A>
where
    A: PartialEq + Eq + Hash,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<A> From<HashMap<A, HashMap<A, HashSet<A>>>> for Remotes<A>
where
    A: PartialEq + Eq + Hash,
{
    fn from(map: HashMap<A, HashMap<A, HashSet<A>>>) -> Self {
        Self::from_map(map)
    }
}

/// The current `refs/heads` and [`Remotes`] (transitive tracking graph)
#[derive(Debug, Serialize, Deserialize)]
pub struct Refs {
    pub heads: BTreeMap<String, Oid>,
    pub remotes: Remotes<PeerId>,
}

impl Refs {
    pub fn sign(self, key: &SecretKey) -> Result<Signed, CjsonError> {
        let signature = key.sign(&self.canonical_form()?);
        Ok(Signed {
            refs: self,
            signature,
        })
    }

    fn canonical_form(&self) -> Result<Vec<u8>, CjsonError> {
        Cjson(self).canonical_form()
    }
}

impl From<Signed> for Refs {
    fn from(sig: Signed) -> Self {
        sig.refs
    }
}

pub mod signed {
    use super::*;

    #[derive(Debug, Error)]
    pub enum Error {
        #[error("Invalid signature")]
        InvalidSignature(Refs),

        #[error(transparent)]
        Json(#[from] serde_json::error::Error),

        #[error(transparent)]
        Cjson(#[from] CjsonError),
    }
}

#[derive(Serialize, Deserialize)]
pub struct Signed {
    refs: Refs,
    signature: Signature,
}

impl Signed {
    pub fn from_json(data: &[u8], signer: &PeerId) -> Result<Self, signed::Error> {
        let this: Self = serde_json::from_slice(data)?;
        let canonical = this.refs.canonical_form()?;
        if this.signature.verify(&canonical, &*signer) {
            Ok(this)
        } else {
            Err(signed::Error::InvalidSignature(this.refs))
        }
    }
}
