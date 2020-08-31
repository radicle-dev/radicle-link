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
    convert::{Infallible, TryFrom},
    fmt::{self, Display},
    ops::{Deref, DerefMut},
    str::FromStr,
};

use serde_bytes::ByteBuf;
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

#[cfg(test)]
use proptest::prelude::*;

pub mod formatter;

/// Types which have a canonical representation
pub trait Canonical {
    type Error;

    fn canonical_form(&self) -> Result<Vec<u8>, Self::Error>;
}

#[derive(Debug, Error)]
#[error(transparent)]
pub struct CjsonError(#[from] serde_json::error::Error);

/// The [Canonical JSON] representation of type `T`
///
/// [Canonical JSON]: http://wiki.laptop.org/go/Canonical_JSON
pub struct Cjson<T>(pub T);

impl<T> Cjson<T> {
    pub fn canonical_form(&self) -> Result<Vec<u8>, CjsonError>
    where
        T: serde::Serialize,
    {
        let mut buf = vec![];
        let mut ser =
            serde_json::Serializer::with_formatter(&mut buf, formatter::CanonicalFormatter::new());
        self.0.serialize(&mut ser)?;
        Ok(buf)
    }

    pub fn from_slice(s: &[u8]) -> Result<Self, CjsonError>
    where
        T: serde::de::DeserializeOwned,
    {
        Self::try_from(s)
    }

    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> Deref for Cjson<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for Cjson<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> Canonical for Cjson<T>
where
    T: serde::Serialize,
{
    type Error = CjsonError;

    fn canonical_form(&self) -> Result<Vec<u8>, Self::Error> {
        self.canonical_form()
    }
}

impl<T> TryFrom<&str> for Cjson<T>
where
    T: serde::de::DeserializeOwned,
{
    type Error = CjsonError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::try_from(s.as_bytes())
    }
}

impl<T> TryFrom<&[u8]> for Cjson<T>
where
    T: serde::de::DeserializeOwned,
{
    type Error = CjsonError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        serde_json::from_slice(bytes)
            .map(Self)
            .map_err(Self::Error::from)
    }
}

impl<T> FromStr for Cjson<T>
where
    T: serde::de::DeserializeOwned,
{
    type Err = CjsonError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s)
    }
}

/// A string type suitable for [Canonical JSON]
///
/// Canonical JSON is not a proper subset of [RFC 7159] JSON, in that it only
/// escapes quotation marks and the backslash ("reverse solidus") in string
/// values. The string is stored in Unicode Normalization Form C (NFC) as per
/// the [Unicode Standard Annex #15].
///
/// This newtype wrapper stores the [`String`] in NFC internally, in order to
/// preserve equivalence: NFC is not reversible, and may result in a different
/// lexicographic ordering. So, wrapping any Rust [`String`] in a [`Cstring`]
/// will make sure it compares equal to a string obtained from a
/// source in canonical form. Note, however, that this means that converting
/// back from a [`Cstring`] may not yield the same input [`String`] (although it
/// will _render_ the same).
///
/// The [`serde::Deserialize`] impl interprets the input as raw bytes, and then
/// performs the conversion. It is thus possible to parse compliant [Canonical
/// JSON], ie. string values containing unescaped control characters.
///
/// [Canonical JSON]: http://wiki.laptop.org/go/Canonical_JSON
/// [RFC 7159]: https://tools.ietf.org/html/rfc7159
/// [Unicode Standard Annex #15]: http://www.unicode.org/reports/tr15/
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, serde::Serialize)]
#[serde(transparent)]
pub struct Cstring(String);

#[cfg(test)]
impl Arbitrary for Cstring {
    type Parameters = ();
    type Strategy = prop::strategy::Map<&'static str, fn(String) -> Self>;

    fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
        ".*".prop_map(|s| Cstring(s.nfc().collect()))
    }
}

impl<'de> serde::Deserialize<'de> for Cstring {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let buf = ByteBuf::deserialize(deserializer)?;
        let s = unsafe { std::str::from_utf8_unchecked(&buf) };
        Ok(Self::from(s))
    }
}

impl Deref for Cstring {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// **Note**: due to unicode normalization, `Cstring::from(s).into() != s`
impl From<String> for Cstring {
    fn from(s: String) -> Self {
        Self::from(s.as_str())
    }
}

/// **Note**: due to unicode normalization, `Cstring::from(s).deref() != s`
impl From<&str> for Cstring {
    fn from(s: &str) -> Self {
        Self(s.nfc().collect())
    }
}

impl FromStr for Cstring {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from(s))
    }
}

impl Display for Cstring {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// **Note**: due to unicode normalization, `Cstring::from(s).into() != s`
impl From<Cstring> for String {
    fn from(Cstring(s): Cstring) -> Self {
        s
    }
}

pub mod string {
    use super::Cstring;

    use serde::Deserialize;

    /// A deserialise function suitable for use with `#[serde(deserialize_with =
    /// "..")]`
    ///
    /// This is useful when it is not desirable or possible to use the
    /// [`Cstring`] wrapper. Note, however, that the resulting [`String`] is
    /// in normal form, and may thus not be equivalent (compare as equal) to
    /// the "same" string create from a Rust literal.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<String, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let Cstring(s) = Cstring::deserialize(deserializer)?;
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use librad_test::roundtrip::*;
    use pretty_assertions::assert_eq;

    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct T {
        #[serde(deserialize_with = "string::deserialize")]
        field: String,
    }

    impl T {
        fn normalised(&self) -> Self {
            Self {
                field: self.field.nfc().collect(),
            }
        }
    }

    fn gen_t() -> impl Strategy<Value = T> {
        ".*".prop_map(|field| T { field })
    }

    proptest! {
        #[test]
        fn cstring_roundtrip_str(cstring in any::<Cstring>()) {
            str_roundtrip(cstring)
        }

        #[test]
        fn cstring_roundtrip_json(cstring in any::<Cstring>()) {
            json_roundtrip(cstring)
        }

        #[test]
        fn cstring_roundtrip_cjson(cstring in any::<Cstring>()) {
            cjson_roundtrip(cstring)
        }

        #[test]
        fn any_string_roundtrip_json(t in gen_t()) {
            let ser = serde_json::to_string(&t).unwrap();
            let de = serde_json::from_str(&ser).unwrap();

            assert_eq!(t.normalised(), de)
        }

        #[test]
        fn any_string_roundtrip_cjson(t in gen_t()) {
            let canonical = Cjson(&t).canonical_form().unwrap();

            assert_eq!(t.normalised(), serde_json::from_slice(&canonical).unwrap())
        }
    }
}
