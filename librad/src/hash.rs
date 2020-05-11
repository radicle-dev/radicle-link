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
    convert::TryFrom,
    fmt::{self, Display},
    str::FromStr,
};

use multibase::Base;
use multihash::{Blake2b256, Multihash};
use serde::{de::Visitor, Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// A hash function, suitable for small inputs
pub trait Hasher: PartialEq + Eq {
    /// Hash the supplied slice
    fn hash(data: &[u8]) -> Self;
}

#[derive(Clone, PartialEq, Eq, Debug, Error)]
#[error("Invalid hash algorithm, expected {expected:?}, actual {actual:?}")]
pub struct AlgorithmMismatch {
    expected: multihash::Code,
    actual: multihash::Code,
}

/// A hash obtained using the default hash function
///
/// Use this type for all hashing needs which don't depend on VCS specifics.
/// Currently, this uses Blake2b-256 for compatibility with `radicle-registry`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Hash(Multihash);

impl Hash {
    pub fn hash(data: &[u8]) -> Self {
        Hash(Blake2b256::digest(data))
    }

    pub fn as_ref(&self) -> HashRef {
        HashRef(&self.0)
    }
}

impl Hasher for Hash {
    fn hash(data: &[u8]) -> Self {
        Self::hash(data)
    }
}

impl TryFrom<Multihash> for Hash {
    type Error = AlgorithmMismatch;

    fn try_from(mh: Multihash) -> Result<Self, Self::Error> {
        HashRef::try_from(&mh).map(|h| h.to_owned())
    }
}

impl Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.as_ref().fmt(f)
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Error)]
pub enum ParseError {
    #[error(transparent)]
    Algo(#[from] AlgorithmMismatch),

    #[error(transparent)]
    Encoding(#[from] multibase::Error),

    #[error(transparent)]
    Multihash(#[from] multihash::DecodeOwnedError),
}

impl FromStr for Hash {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = multibase::decode(s).map(|(_base, bytes)| bytes)?;
        let mhash = Multihash::from_bytes(bytes)?;
        Self::try_from(mhash).map_err(|e| e.into())
    }
}

impl Serialize for Hash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Hash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct HashVisitor;

        impl<'de> Visitor<'de> for HashVisitor {
            type Value = Hash;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a Hash")
            }

            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                s.parse().map_err(serde::de::Error::custom)
            }
        }

        deserializer.deserialize_str(HashVisitor)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct HashRef<'a>(&'a Multihash);

impl<'a> HashRef<'a> {
    pub fn to_owned(&self) -> Hash {
        Hash(self.0.clone())
    }
}

impl<'a> TryFrom<&'a Multihash> for HashRef<'a> {
    type Error = AlgorithmMismatch;

    fn try_from(mh: &'a Multihash) -> Result<HashRef<'a>, Self::Error> {
        match mh.algorithm() {
            multihash::Code::Blake2b256 => Ok(Self(mh)),
            c => Err(AlgorithmMismatch {
                expected: multihash::Code::Blake2b256,
                actual: c,
            }),
        }
    }
}

impl<'a> Display for HashRef<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&multibase::encode(Base::Base32Z, self.0))
    }
}

#[cfg(test)]
mod fast {
    use std::{
        fmt::{self, Display},
        hash::Hasher,
        num::ParseIntError,
        str::FromStr,
    };

    use fnv::FnvHasher;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// A fast, but not cryptographically secure hash function
    ///
    /// **Only** use this in test code which does not rely on collision
    /// resistance properties of the hash function.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct FastHash(u64);

    impl super::Hasher for FastHash {
        fn hash(data: &[u8]) -> Self {
            let mut hasher = FnvHasher::default();
            hasher.write(data);
            Self(hasher.finish())
        }
    }

    impl Display for FastHash {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl FromStr for FastHash {
        type Err = ParseIntError;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            s.parse().map(Self)
        }
    }

    impl Serialize for FastHash {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_u64(self.0)
        }
    }

    impl<'de> Deserialize<'de> for FastHash {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            u64::deserialize(deserializer).map(Self)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{fast::*, *};

    use std::fmt::Debug;

    use rand::random;

    fn is_a_deterministic_function<H: Hasher + Debug>() {
        let data: [u8; 32] = random();
        assert_eq!(H::hash(&data), H::hash(&data))
    }

    fn can_serde<H>()
    where
        for<'de> H: Hasher + Debug + Serialize + Deserialize<'de>,
    {
        let data: [u8; 32] = random();
        let hash = H::hash(&data);

        let json = serde_json::to_string(&hash).unwrap();
        let de1 = serde_json::from_str(&json).unwrap();

        let cbor = serde_cbor::to_vec(&hash).unwrap();
        let de2 = serde_cbor::from_slice(&cbor).unwrap();

        assert_eq!(de1, de2);
        assert_eq!(hash, de1);
    }

    fn can_display_from_str<H>()
    where
        H: Hasher + Debug + Display + FromStr,
        H::Err: Debug,
    {
        let data: [u8; 32] = random();
        let hash1 = H::hash(&data);
        let hash2 = hash1.to_string().parse().unwrap();
        assert_eq!(hash1, hash2)
    }

    #[test]
    fn test_determinism() {
        is_a_deterministic_function::<Hash>();
        is_a_deterministic_function::<FastHash>();
    }

    #[test]
    fn test_serde() {
        can_serde::<Hash>();
        can_serde::<FastHash>();
    }

    #[test]
    fn test_serde_wrong_algorithm() {
        let data: [u8; 32] = random();

        let sha3 = multibase::encode(Base::Base32Z, multihash::Sha3_256::digest(&data));

        let json = serde_json::to_string(&sha3).unwrap();
        let de: Result<Hash, serde_json::Error> = serde_json::from_str(&json);

        // Bravo, serde: the std::error::Error impls only return a `source()`
        // for IO errors. So no option but to match against the `Display` impl.
        // Sorry, future maintainer!
        let expect_err = de.unwrap_err().to_string();
        assert!(
            expect_err.starts_with("Invalid hash algorithm, expected Blake2b256, actual Sha3_256")
        )
    }

    #[test]
    fn test_str_roundtrip() {
        can_display_from_str::<Hash>();
        can_display_from_str::<FastHash>();
    }
}
