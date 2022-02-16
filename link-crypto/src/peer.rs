// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom, fmt, ops::Deref, str::FromStr};

use git_ext as ext;
use minicbor::{Decode, Encode};
use multibase::Base::Base32Z;
use serde::{de::Visitor, Deserialize, Deserializer, Serialize, Serializer};

use keystore::sign;

use crate::keys::{self, PublicKey, SecretKey};

#[derive(Clone, Copy, Eq, PartialEq, PartialOrd, Ord, Hash, Encode, Decode)]
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

        PublicKey::from_slice(key)
            .map(PeerId)
            .ok_or(InvalidPublicKey)
    }

    pub fn as_dns_name(&self) -> webpki::DNSName {
        (*self).into()
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
                PeerId::from_str(s).map_err(serde::de::Error::custom)
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

impl fmt::Debug for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
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

impl From<PeerId> for webpki::DNSName {
    fn from(peer_id: PeerId) -> Self {
        webpki::DNSNameRef::try_from_ascii_str(&peer_id.to_string())
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

impl From<&PeerId> for ext::RefLike {
    fn from(peer_id: &PeerId) -> Self {
        Self::try_from(peer_id.to_string())
            .expect("string representation of PeerId must be a valid git refname")
    }
}

impl From<PeerId> for ext::RefLike {
    fn from(peer_id: PeerId) -> Self {
        (&peer_id).into()
    }
}

#[cfg(feature = "git-ref-format")]
impl From<&PeerId> for git_ref_format::Component<'_> {
    #[inline]
    fn from(id: &PeerId) -> Self {
        use git_ref_format::{Component, RefString};

        let refstr = RefString::try_from(id.to_string()).expect("`PeerId` is a valid ref string");
        Component::from_refstring(refstr).expect("`PeerId` is a valid refname component")
    }
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub struct OriginatesRef<'a, T> {
    pub from: &'a PeerId,
    pub value: &'a T,
}

impl<'a, T> From<&'a Originates<T>> for OriginatesRef<'a, T> {
    fn from(orig: &'a Originates<T>) -> Self {
        orig.as_ref()
    }
}
