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

use std::fmt;

use serde::{
    de::{MapAccess, Visitor},
    Deserialize,
    Deserializer,
    Serialize,
    Serializer,
};

use crate::{
    hash::Hash,
    uri::{self, RadUrn},
};

#[derive(Clone, Debug, PartialEq)]
pub enum Rev {
    Git(git2::Oid),
}

impl Rev {
    pub fn as_proto(&self) -> &uri::Protocol {
        self.into()
    }

    pub fn into_proto(self) -> uri::Protocol {
        self.into()
    }
}

// FIXME: Below is standard CBOR tuple-encoding. We should really use a proper
// CBOR library instead of serde (cf. #102)
impl Serialize for Rev {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Git(oid) => {
                let mut seq = vec![0];
                seq.extend_from_slice(oid.as_bytes());
                seq.serialize(serializer)
            },
        }
    }
}

impl<'de> Deserialize<'de> for Rev {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let x = Vec::deserialize(deserializer)?;
        match x.split_first() {
            Some((tag, oid)) => match tag {
                0 => git2::Oid::from_bytes(oid)
                    .map(Self::Git)
                    .map_err(serde::de::Error::custom),
                _ => Err(serde::de::Error::custom("Unknown tag")),
            },

            None => Err(serde::de::Error::custom("No data")),
        }
    }
}

impl Into<uri::Protocol> for Rev {
    fn into(self) -> uri::Protocol {
        match self {
            Self::Git(_) => uri::Protocol::Git,
        }
    }
}

impl<'a> Into<&'a uri::Protocol> for &'a Rev {
    fn into(self) -> &'a uri::Protocol {
        match self {
            Rev::Git(_) => &uri::Protocol::Git,
        }
    }
}

/// The gossip payload type
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Gossip {
    /// URN of an updated or wanted repo.
    ///
    /// _May_ include the branch name as the `path` component.
    pub urn: RadUrn,

    /// The revision advertised or wanted
    pub rev: Rev,
}

impl Gossip {
    pub fn new(id: Hash, path: uri::Path, rev: Rev) -> Self {
        Self {
            urn: RadUrn::new(id, rev.clone().into(), path),
            rev,
        }
    }
}

impl<'de> Deserialize<'de> for Gossip {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "lowercase")]
        enum Field {
            Urn,
            Rev,
        }

        struct GossipVisitor;

        impl<'de> Visitor<'de> for GossipVisitor {
            type Value = Gossip;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "struct Gossip")
            }

            fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut urn = None;
                let mut rev = None;
                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Urn => {
                            if urn.is_some() {
                                return Err(serde::de::Error::duplicate_field("urn"));
                            }
                            urn = Some(map.next_value()?);
                        },

                        Field::Rev => {
                            if rev.is_some() {
                                return Err(serde::de::Error::duplicate_field("rev"));
                            }
                            rev = Some(map.next_value()?);
                        },
                    }
                }

                let urn: RadUrn = urn.ok_or_else(|| serde::de::Error::missing_field("urn"))?;
                let rev: Rev = rev.ok_or_else(|| serde::de::Error::missing_field("rev"))?;

                if &urn.proto != rev.as_proto() {
                    Err(serde::de::Error::custom("protocol mismatch"))
                } else {
                    Ok(Gossip { urn, rev })
                }
            }
        }

        const FIELDS: &[&str] = &["urn", "rev"];
        deserializer.deserialize_struct("Gossip", FIELDS, GossipVisitor)
    }
}
