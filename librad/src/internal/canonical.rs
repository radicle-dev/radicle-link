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
    convert::Infallible,
    fmt::{self, Display},
    ops::Deref,
    str::FromStr,
};

use serde::Serialize;
use serde_bytes::ByteBuf;
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

/// Types which have a canonical representation
pub trait Canonical {
    type Error;

    fn canonical_form(&self) -> Result<Vec<u8>, Self::Error>;
}

#[derive(Debug, Error)]
#[error(transparent)]
pub struct CjsonError(#[from] serde_json::error::Error);

/// The canonical JSON representation of type `T`
pub struct Cjson<T>(pub T);

impl<T> Cjson<T>
where
    T: Serialize,
{
    pub fn canonical_form(&self) -> Result<Vec<u8>, CjsonError> {
        let mut buf = vec![];
        let mut ser =
            serde_json::Serializer::with_formatter(&mut buf, olpc_cjson::CanonicalFormatter::new());
        self.0.serialize(&mut ser)?;
        Ok(buf)
    }
}

impl<T> Canonical for Cjson<T>
where
    T: Serialize,
{
    type Error = CjsonError;

    fn canonical_form(&self) -> Result<Vec<u8>, Self::Error> {
        self.canonical_form()
    }
}

/// A string type suitable for [Canonical JSON]
///
/// Canonical JSON is not a proper subset of [RFC 7159] JSON, in that it only
/// escapes quotation marks and the backslash ("reverse solidus") in string
/// values. The string is stored in Unicode Normalization Form C (NFC) as per
/// the [Unicode Standard Annex #15].
///
/// In order to make [`serde_json`] parse JSON containing such canonical
/// strings, we need to go through [`serde_bytes::ByteBuf`]. To ensure
/// correctness of the [`PartialEq`] impl, we store the string in NFC
/// internally.
///
/// Note, however, that [`serde_json`] is not able to handle control characters
/// in strings (which Canonical JSON allows). Accordingly, the [`Arbitrary`]
/// instance doesn't generate those.
///
/// [Canonical JSON]: http://wiki.laptop.org/go/Canonical_JSON
/// [RFC 7159]: https://tools.ietf.org/html/rfc7159
/// [Unicode Standard Annex #15]: http://www.unicode.org/reports/tr15/
/// [`Arbitrary`]: https://docs.rs/proptest/0.10.0/proptest/arbitrary/trait.Arbitrary.html
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(transparent)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct Cstring(#[cfg_attr(test, proptest(strategy(gen_string_nfc)))] String);

#[cfg(test)]
pub fn gen_string_nfc() -> impl proptest::strategy::Strategy<Value = String> {
    use proptest::prelude::*;

    "\\P{Cc}*".prop_map(|s| s.nfc().collect())
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

impl From<String> for Cstring {
    fn from(s: String) -> Self {
        Self::from(s.as_str())
    }
}

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
    use proptest::prelude::*;
    use proptest_derive::Arbitrary;

    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize, Arbitrary)]
    struct T {
        #[serde(deserialize_with = "string::deserialize")]
        #[proptest(regex = "\\P{Cc}*")]
        field: String,
    }

    impl T {
        fn normalised(&self) -> Self {
            Self {
                field: self.field.nfc().collect(),
            }
        }
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
            let canonical = Cjson(&cstring).canonical_form().unwrap();

            assert_eq!(cstring, serde_json::from_slice(&canonical).unwrap())
        }

        #[test]
        fn any_string_roundtrip_json(t in any::<T>()) {
            let ser = serde_json::to_string(&t).unwrap();
            let de = serde_json::from_str(&ser).unwrap();

            assert_eq!(t.normalised(), de)
        }

        #[test]
        fn any_string_roundtrip_cjson(t in any::<T>()) {
            let canonical = Cjson(&t).canonical_form().unwrap();

            assert_eq!(t.normalised(), serde_json::from_slice(&canonical).unwrap())
        }
    }
}
