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

use std::{convert::TryFrom, fmt, ops::Deref, str::FromStr};

use git_ext as ext;
use minicbor::{Decode, Encode};
use multibase::Base::Base32Z;
use serde::{de::Visitor, Deserialize, Deserializer, Serialize, Serializer};

use keystore::sign;

use crate::keys::{self, PublicKey, SecretKey};

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Hash, Encode, Decode)]
#[cbor(array)]
pub struct PeerId(#[n(0)] PublicKey);

impl PeerId {
    pub fn as_public_key(&self) -> &PublicKey {
        &self.0
    }

    /// Canonical representation of a `PeerId`
    ///
    /// This is the `multibase` encoding, using the `z-base32` alphabet, of the
    /// public key bytes prepended by a one byte reserved value.
    pub fn default_encoding(&self) -> String {
        let mut buf = [0u8; keys::PUBLICKEYBYTES + 1];
        buf[1..].copy_from_slice(&self.0.as_ref()[0..]);

        multibase::encode(Base32Z, &buf[0..])
    }

    const ENCODED_LEN: usize = 1 + (((keys::PUBLICKEYBYTES + 1) * 8 + 4) / 5); // 54

    /// Attempt to deserialise from the canonical representation
    pub fn from_default_encoding(s: &str) -> Result<Self, conversion::Error> {
        use conversion::Error::*;

        if s.len() != Self::ENCODED_LEN {
            return Err(UnexpectedInputLength(s.len()));
        }

        let (_, bytes) = multibase::decode(s)?;
        let (version, key) = bytes
            .split_first()
            .expect("We check the input length, therefore there must be data. qed");

        if *version != 0 {
            return Err(UnknownVersion(*version));
        }

        PublicKey::from_slice(&key)
            .map(PeerId)
            .ok_or_else(|| InvalidPublicKey)
    }

    pub fn as_dns_name(&self) -> webpki::DNSName {
        self.clone().into()
    }

    pub fn from_signer(signer: &impl sign::Signer) -> Self {
        PeerId(signer.public_key().into())
    }
}

impl From<PublicKey> for PeerId {
    fn from(pk: PublicKey) -> Self {
        Self(pk)
    }
}

impl From<SecretKey> for PeerId {
    fn from(k: SecretKey) -> Self {
        Self(k.public())
    }
}

impl From<&SecretKey> for PeerId {
    fn from(k: &SecretKey) -> Self {
        Self(k.public())
    }
}

impl Serialize for PeerId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for PeerId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct PeerIdVisitor;

        impl<'de> Visitor<'de> for PeerIdVisitor {
            type Value = PeerId;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "A PeerId")
            }

            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                PeerId::from_str(&s).map_err(serde::de::Error::custom)
            }
        }

        deserializer.deserialize_str(PeerIdVisitor)
    }
}

pub mod conversion {
    use thiserror::Error;

    #[derive(Debug, Error)]
    #[non_exhaustive]
    pub enum Error {
        #[error("unexpected input length: {0}")]
        UnexpectedInputLength(usize),

        #[error("unknown version: {0}")]
        UnknownVersion(u8),

        #[error("invalid public key")]
        InvalidPublicKey,

        #[error("decode error: {0}")]
        DecodeError(#[from] multibase::Error),
    }
}

impl FromStr for PeerId {
    type Err = conversion::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_default_encoding(s)
    }
}

impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.default_encoding())
    }
}

impl<'a> TryFrom<webpki::DNSNameRef<'a>> for PeerId {
    type Error = webpki::Error;

    fn try_from(dns_name: webpki::DNSNameRef) -> Result<Self, Self::Error> {
        let dns_name: &str = dns_name.into();
        PeerId::from_str(dns_name).map_err(|e| {
            tracing::trace!(msg = "PeerId::from_str failed", dns.name = %dns_name, error = %e);
            webpki::Error::NameConstraintViolation
        })
    }
}

impl Into<webpki::DNSName> for PeerId {
    fn into(self) -> webpki::DNSName {
        webpki::DNSNameRef::try_from_ascii_str(&self.to_string())
            .unwrap()
            .to_owned()
    }
}

impl Deref for PeerId {
    type Target = PublicKey;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// FIXME(kim): We'd probably want From impls, instead of only Into, but the
// typechecker fails to terminate in some cases. No clue why.

impl Into<ext::RefLike> for &PeerId {
    fn into(self) -> ext::RefLike {
        ext::RefLike::try_from(self.to_string())
            .expect("string representation of PeerId must be a valid git refname")
    }
}

impl Into<ext::RefLike> for PeerId {
    fn into(self) -> ext::RefLike {
        (&self).into()
    }
}

pub struct Originates<T> {
    pub from: PeerId,
    pub value: T,
}

impl<T> Originates<T> {
    pub fn as_ref(&'_ self) -> OriginatesRef<'_, T> {
        OriginatesRef {
            from: &self.from,
            value: &self.value,
        }
    }
}

pub struct OriginatesRef<'a, T> {
    pub from: &'a PeerId,
    pub value: &'a T,
}

impl<'a, T> From<&'a Originates<T>> for OriginatesRef<'a, T> {
    fn from(orig: &'a Originates<T>) -> Self {
        orig.as_ref()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    use librad_test::roundtrip::*;

    #[test]
    fn test_default_encoding_roundtrip() {
        let peer_id1 = PeerId::from(SecretKey::new().public());
        let peer_id2 = PeerId::from_default_encoding(&peer_id1.default_encoding()).unwrap();

        assert_eq!(peer_id1, peer_id2)
    }

    #[test]
    fn test_default_encoding_empty_input() {
        assert!(matches!(
            PeerId::from_default_encoding(""),
            Err(conversion::Error::UnexpectedInputLength(0))
        ))
    }

    #[test]
    fn test_str_roundtrip() {
        str_roundtrip(PeerId::from(SecretKey::new().public()));
    }

    #[test]
    fn test_cbor_roundtrip() {
        cbor_roundtrip(PeerId::from(SecretKey::new().public()))
    }

    #[test]
    fn test_dns_name_roundtrip() {
        let peer_id1 = PeerId::from(SecretKey::new());
        let dns_name: webpki::DNSName = peer_id1.clone().into();
        let peer_id2 = PeerId::try_from(dns_name.as_ref()).unwrap();

        assert_eq!(peer_id1, peer_id2)
    }

    #[test]
    fn peerid_is_reflike() {
        let peer_id = PeerId::from(SecretKey::new());
        assert_eq!(
            &peer_id.to_string(),
            Into::<ext::RefLike>::into(&peer_id).as_str()
        )
    }
}
