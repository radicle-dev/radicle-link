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
    peer::PeerId,
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
    /// The path component denotes the named branch the `rev` was applied to.
    /// Defaults to `rad/id` if empty.
    pub urn: RadUrn,

    /// The revision advertised or wanted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rev: Option<Rev>,

    /// The origin of the update.
    pub origin: PeerId,
}

impl Gossip {
    pub fn new(id: Hash, path: uri::Path, rev: impl Into<Option<Rev>>, origin: PeerId) -> Self {
        let rev = rev.into();
        // FIXME: we really need the uri protocol on the type level
        let urn = RadUrn::new(id, uri::Protocol::Git, path);

        Self { urn, rev, origin }
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
            Origin,
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
                let mut urn: Option<RadUrn> = None;
                let mut rev: Option<Rev> = None;
                let mut origin: Option<PeerId> = None;

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

                        Field::Origin => {
                            if origin.is_some() {
                                return Err(serde::de::Error::duplicate_field("origin"));
                            }
                            origin = Some(map.next_value()?);
                        },
                    }
                }

                let urn: RadUrn = urn.ok_or_else(|| serde::de::Error::missing_field("urn"))?;
                let origin: PeerId =
                    origin.ok_or_else(|| serde::de::Error::missing_field("origin"))?;

                if let Some(ref rev) = rev {
                    if &urn.proto != rev.as_proto() {
                        return Err(serde::de::Error::custom("protocol mismatch"));
                    }
                }
                Ok(Gossip { urn, rev, origin })
            }
        }

        const FIELDS: &[&str] = &["urn", "rev", "origin"];
        deserializer.deserialize_struct("Gossip", FIELDS, GossipVisitor)
    }
}
