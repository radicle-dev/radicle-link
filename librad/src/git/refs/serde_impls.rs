// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use super::{Refs, Remotes};

use crypto::PeerId;
use git_ext::Oid;
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
                let mut categorised_refs: BTreeMap<String, BTreeMap<String, Oid>> = BTreeMap::new();

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "remotes" => {
                            let value = map.next_value()?;
                            remotes = Some(value);
                        },
                        _ => {
                            let value = map.next_value()?;
                            categorised_refs.insert(key, value);
                        },
                    }
                }
                let remotes = remotes.ok_or_else(|| serde::de::Error::missing_field("remotes"))?;
                Ok(Refs {
                    remotes,
                    categorised_refs,
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
        for (category, values) in &self.categorised_refs {
            map_s.serialize_entry(category, &values)?;
        }
        map_s.serialize_entry("remotes", &self.remotes)?;
        map_s.end()
    }
}
