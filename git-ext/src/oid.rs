// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    convert::TryFrom,
    fmt::{self, Display},
    ops::Deref,
    str::FromStr,
};

use multihash::{Multihash, MultihashRef};
use thiserror::Error;

/// Serializable [`git2::Oid`]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Oid(git2::Oid);

impl Oid {
    pub fn into_multihash(self) -> Multihash {
        self.into()
    }
}

#[cfg(feature = "serde")]
mod serde_impls {
    use super::*;
    use serde::{de::Visitor, Deserialize, Deserializer, Serialize, Serializer};

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
}

#[cfg(feature = "minicbor")]
mod minicbor_impls {
    use super::*;
    use minicbor::{
        decode,
        encode::{self, Write},
        Decode,
        Decoder,
        Encode,
        Encoder,
    };

    impl Encode for Oid {
        fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), encode::Error<W::Error>> {
            e.bytes(Multihash::from(self).as_bytes())?;
            Ok(())
        }
    }

    impl<'b> Decode<'b> for Oid {
        fn decode(d: &mut Decoder) -> Result<Self, decode::Error> {
            let bytes = d.bytes()?;
            let mhash = MultihashRef::from_slice(bytes)
                .or(Err(decode::Error::Message("not a multihash")))?;
            Self::try_from(mhash).or(Err(decode::Error::Message("not a git oid")))
        }
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

#[cfg(feature = "git-hash")]
impl AsRef<git_hash::oid> for Oid {
    fn as_ref(&self) -> &git_hash::oid {
        // SAFETY: checks the length of the slice, which we know is correct
        git_hash::oid::try_from(self.as_bytes()).unwrap()
    }
}

impl From<git2::Oid> for Oid {
    fn from(oid: git2::Oid) -> Self {
        Self(oid)
    }
}

impl From<Oid> for git2::Oid {
    fn from(oid: Oid) -> Self {
        oid.0
    }
}

#[cfg(feature = "git-hash")]
impl From<git_hash::ObjectId> for Oid {
    fn from(git_hash::ObjectId::Sha1(bs): git_hash::ObjectId) -> Self {
        // SAFETY: checks the length of the slice, which we statically know
        Self(git2::Oid::from_bytes(&bs).unwrap())
    }
}

#[cfg(feature = "git-hash")]
impl From<Oid> for git_hash::ObjectId {
    fn from(oid: Oid) -> Self {
        Self::from_20_bytes(oid.as_ref())
    }
}

#[cfg(feature = "git-hash")]
impl<'a> From<&'a Oid> for &'a git_hash::oid {
    fn from(oid: &'a Oid) -> Self {
        oid.as_ref()
    }
}

impl Display for Oid {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
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
#[non_exhaustive]
pub enum FromMultihashError {
    #[error("invalid hash algorithm: expected Sha1, got {actual:?}")]
    AlgorithmMismatch { actual: multihash::Code },

    #[error(transparent)]
    Git(#[from] git2::Error),
}

impl TryFrom<Multihash> for Oid {
    type Error = FromMultihashError;

    fn try_from(mhash: Multihash) -> Result<Self, Self::Error> {
        Self::try_from(mhash.as_ref())
    }
}

impl TryFrom<MultihashRef<'_>> for Oid {
    type Error = FromMultihashError;

    fn try_from(mhash: MultihashRef) -> Result<Self, Self::Error> {
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
