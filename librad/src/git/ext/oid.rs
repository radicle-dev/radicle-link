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

use std::{convert::TryFrom, fmt, ops::Deref, str::FromStr};

use multihash::Multihash;
use serde::{de::Visitor, Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// Serializable [`git2::Oid`]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
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
                write!(f, "a hexidecimal git2::Oid")
            }

            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                s.parse().map_err(serde::de::Error::custom)
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

impl AsRef<git2::Oid> for Oid {
    fn as_ref(&self) -> &git2::Oid {
        self
    }
}

impl AsRef<[u8]> for Oid {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl From<git2::Oid> for Oid {
    fn from(oid: git2::Oid) -> Self {
        Self(oid)
    }
}

impl TryFrom<&str> for Oid {
    type Error = git2::Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse().map(Self)
    }
}

impl FromStr for Oid {
    type Err = git2::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s)
    }
}

#[derive(Debug, Error)]
pub enum FromMultihashError {
    #[error("Invalid hash algorithm: expected Sha1, got {actual:?}")]
    AlgorithmMismatch { actual: multihash::Code },

    #[error(transparent)]
    Git(#[from] git2::Error),
}

impl TryFrom<Multihash> for Oid {
    type Error = FromMultihashError;

    fn try_from(mhash: Multihash) -> Result<Self, Self::Error> {
        if mhash.algorithm() != multihash::Code::Sha1 {
            return Err(Self::Error::AlgorithmMismatch {
                actual: mhash.algorithm(),
            });
        }

        Self::try_from(mhash.digest()).map_err(Self::Error::from)
    }
}

impl TryFrom<&[u8]> for Oid {
    type Error = git2::Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        git2::Oid::from_bytes(bytes).map(Self)
    }
}

impl From<Oid> for Multihash {
    fn from(oid: Oid) -> Self {
        Self::from(&oid)
    }
}

impl From<&Oid> for Multihash {
    fn from(oid: &Oid) -> Self {
        multihash::wrap(multihash::Code::Sha1, oid.as_ref())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    use proptest::prelude::*;

    pub fn gen_oid(kind: git2::ObjectType) -> impl Strategy<Value = Oid> {
        any::<Vec<u8>>()
            .prop_map(move |bytes| git2::Oid::hash_object(kind, &bytes).map(Oid::from).unwrap())
    }
}
