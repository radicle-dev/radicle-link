// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use minicbor::{Decode, Decoder, Encode, Encoder};

use crate::{identities::git::Urn, peer::PeerId};

use serde::Serialize;

#[derive(Clone, Debug, PartialEq)]
pub enum Rev {
    Git(git2::Oid),
}

impl From<git2::Oid> for Rev {
    fn from(oid: git2::Oid) -> Self {
        Self::Git(oid)
    }
}

impl From<git_ext::Oid> for Rev {
    fn from(oid: git_ext::Oid) -> Self {
        Self::Git(oid.into())
    }
}

impl Encode for Rev {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut Encoder<W>,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        let Self::Git(oid) = self;

        e.array(2)?.u32(0)?.bytes(oid.as_ref())?;

        Ok(())
    }
}

impl<'de> Decode<'de> for Rev {
    fn decode(d: &mut Decoder<'de>) -> Result<Self, minicbor::decode::Error> {
        if Some(2) != d.array()? {
            return Err(minicbor::decode::Error::Message("expected 2-element array"));
        }

        match d.u32()? {
            0 => {
                let bytes = d.bytes()?;
                git2::Oid::from_bytes(bytes)
                    .map(Self::Git)
                    .map_err(|_| minicbor::decode::Error::Message("invalid git oid"))
            },
            n => Err(minicbor::decode::Error::UnknownVariant(n)),
        }
    }
}

impl Serialize for Rev {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Git(oid) => serializer.serialize_str(oid.to_string().as_str()),
        }
    }
}

/// The gossip payload type
#[derive(Clone, Debug, PartialEq, Encode, Decode, Serialize)]
#[cbor(array)]
pub struct Payload {
    /// URN of an updated or wanted repo.
    ///
    /// The path component denotes the named branch the `rev` was applied to.
    /// Defaults to `rad/id` if empty.
    #[n(0)]
    pub urn: Urn,

    /// The revision advertised or wanted.
    #[n(1)]
    pub rev: Option<Rev>,

    /// The origin of the update.
    ///
    /// If `Some`, this refers to the `PeerId`'s view of `urn` and `rev`. That
    /// is, it may map to `remotes/<origin>/<urn.path@rev>`.
    #[n(2)]
    pub origin: Option<PeerId>,
}
