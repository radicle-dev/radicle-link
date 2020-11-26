// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use minicbor::{Decode, Decoder, Encode, Encoder};

use crate::{identities::git::Urn, peer::PeerId};

#[derive(Clone, Debug, PartialEq)]
pub enum Rev {
    Git(git2::Oid),
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

/// The gossip payload type
#[derive(Clone, Debug, PartialEq, Encode, Decode)]
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
    /// is, it may map to `remotes/<PeerId>/<urn>`.
    #[n(2)]
    pub origin: Option<PeerId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{keys::SecretKey, peer::PeerId};
    use librad_test::roundtrip::*;

    lazy_static! {
        static ref OID: git2::Oid =
            git2::Oid::hash_object(git2::ObjectType::Commit, b"chrzbrr").unwrap();
    }

    #[test]
    fn roundtrip_rev() {
        cbor_roundtrip(Rev::Git(*OID));
    }

    #[test]
    fn roundtrip_payload() {
        let payload = Payload {
            urn: Urn::new(git_ext::Oid::from(git2::Oid::zero())),
            rev: Some(Rev::Git(*OID)),
            origin: Some(PeerId::from(SecretKey::new())),
        };

        cbor_roundtrip(payload)
    }
}
