// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use super::{Refs, Remotes};

use crypto::PeerId;
use git_ext::{reference, Oid};
use serde::ser::SerializeMap;

use std::collections::BTreeMap;

impl<'de> serde::Deserialize<'de> for Refs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct RefsVisitor;

        impl<'vde> serde::de::Visitor<'vde> for RefsVisitor {
            type Value = Refs;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a map containing a \"refs\" and a \"remotes\" key")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'vde>,
            {
                let mut remotes: Option<Remotes<PeerId>> = None;
                let mut heads: Option<BTreeMap<reference::OneLevel, Oid>> = None;
                let mut rad: Option<BTreeMap<reference::OneLevel, Oid>> = None;
                let mut tags: Option<BTreeMap<reference::OneLevel, Oid>> = None;
                let mut notes: Option<BTreeMap<reference::OneLevel, Oid>> = None;
                let mut cobs: Option<BTreeMap<reference::OneLevel, Oid>> = None;
                let mut unknown: BTreeMap<String, BTreeMap<String, Oid>> = BTreeMap::new();

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "heads" => {
                            let value = map.next_value()?;
                            heads = Some(value);
                        },
                        "rad" => {
                            let value = map.next_value()?;
                            rad = Some(value);
                        },
                        "tags" => {
                            let value = map.next_value()?;
                            tags = Some(value);
                        },
                        "notes" => {
                            let value = map.next_value()?;
                            notes = Some(value);
                        },
                        "cobs" => {
                            let value = map.next_value()?;
                            cobs = Some(value);
                        },
                        "remotes" => {
                            let value = map.next_value()?;
                            remotes = Some(value);
                        },
                        _ => {
                            let value = map.next_value()?;
                            unknown.insert(key, value);
                        },
                    }
                }
                let remotes = remotes.ok_or_else(|| serde::de::Error::missing_field("remotes"))?;
                let heads = heads.ok_or_else(|| serde::de::Error::missing_field("heads"))?;
                let rad = rad.ok_or_else(|| serde::de::Error::missing_field("rad"))?;
                let tags = tags.ok_or_else(|| serde::de::Error::missing_field("tags"))?;
                let notes = notes.ok_or_else(|| serde::de::Error::missing_field("notes"))?;
                Ok(Refs {
                    heads,
                    rad,
                    tags,
                    notes,
                    cobs,
                    remotes,
                    unknown_categories: unknown,
                })
            }
        }
        deserializer.deserialize_map(RefsVisitor)
    }
}

impl serde::Serialize for Refs {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map_s = serializer.serialize_map(Some(6))?;
        map_s.serialize_entry("heads", &self.heads)?;
        map_s.serialize_entry("rad", &self.rad)?;
        map_s.serialize_entry("tags", &self.tags)?;
        map_s.serialize_entry("notes", &self.notes)?;
        if let Some(cob) = &self.cobs {
            map_s.serialize_entry("cobs", cob)?;
        }
        for (category, values) in &self.unknown_categories {
            map_s.serialize_entry(category, &values)?;
        }
        map_s.serialize_entry("remotes", &self.remotes)?;
        map_s.end()
    }
}
