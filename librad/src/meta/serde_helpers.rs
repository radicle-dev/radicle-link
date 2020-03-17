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

pub(crate) mod nonempty {
    use nonempty::NonEmpty;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<T, S>(ne: &NonEmpty<T>, serializer: S) -> Result<S::Ok, S::Error>
    where
        T: Serialize,
        S: Serializer,
    {
        let v: Vec<&T> = ne.iter().collect();
        v.serialize(serializer)
    }

    pub fn deserialize<'de, T, D>(deserializer: D) -> Result<NonEmpty<T>, D::Error>
    where
        T: Deserialize<'de> + Clone,
        D: Deserializer<'de>,
    {
        let v = Vec::deserialize(deserializer)?;
        NonEmpty::from_slice(&v).ok_or_else(|| serde::de::Error::custom("Empty list"))
    }
}

pub(crate) mod urltemplate {
    use serde::{Deserialize, Deserializer, Serializer};
    use urltemplate::UrlTemplate;

    pub fn serialize<S>(tpl: &UrlTemplate, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&tpl.to_string())
    }

    pub fn serialize_opt<S>(opt: &Option<UrlTemplate>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if let Some(tpl) = opt {
            serialize(tpl, serializer)
        } else {
            serializer.serialize_none()
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<UrlTemplate, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(UrlTemplate::from(s))
    }

    pub fn deserialize_opt<'de, D>(deserializer: D) -> Result<Option<UrlTemplate>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wrapper(#[serde(deserialize_with = "deserialize")] UrlTemplate);

        let v = Option::deserialize(deserializer)?;
        Ok(v.map(|Wrapper(a)| a))
    }
}

pub(crate) mod pgp_fingerprint {
    use pgp::Fingerprint;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(fingerprint: &Fingerprint, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&fingerprint.to_hex())
    }

    pub fn serialize_opt<S>(opt: &Option<Fingerprint>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if let Some(fingerprint) = opt {
            serialize(fingerprint, serializer)
        } else {
            serializer.serialize_none()
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Fingerprint, D::Error>
    where
        D: Deserializer<'de>,
    {
        let string = String::deserialize(deserializer)?;
        Fingerprint::from_hex(&string).map_err(serde::de::Error::custom)
    }

    pub fn deserialize_opt<'de, D>(deserializer: D) -> Result<Option<Fingerprint>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wrapper(#[serde(deserialize_with = "deserialize")] Fingerprint);

        let v = Option::deserialize(deserializer)?;
        Ok(v.map(|Wrapper(a)| a))
    }
}
