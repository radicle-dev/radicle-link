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

use minicbor::{Decode, Decoder, Encode, Encoder};

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
#[derive(Clone, Debug, PartialEq, Encode, Decode)]
#[cbor(array)]
pub struct Gossip {
    /// URN of an updated or wanted repo.
    ///
    /// The path component denotes the named branch the `rev` was applied to.
    /// Defaults to `rad/id` if empty.
    #[n(0)]
    pub urn: RadUrn,

    /// The revision advertised or wanted.
    #[n(1)]
    pub rev: Option<Rev>,

    /// The origin of the update.
    #[n(2)]
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

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{hash::Hash, keys::SecretKey, peer::PeerId, uri::Path};
    use librad_test::roundtrip::*;

    lazy_static! {
        static ref OID: git2::Oid =
            git2::Oid::hash_object(git2::ObjectType::Commit, b"chrzbrr").unwrap();
    }

    #[test]
    fn test_rev_cbor() {
        cbor_roundtrip(Rev::Git(*OID));
    }

    #[test]
    fn test_gossip_cbor() {
        let gossip = Gossip::new(
            Hash::hash(b"cerveza coronita"),
            Path::new(),
            Rev::Git(*OID),
            PeerId::from(SecretKey::new()),
        );

        cbor_roundtrip(gossip)
    }
}
