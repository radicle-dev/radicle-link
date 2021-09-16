// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    convert::{Infallible, TryFrom},
    error,
    fmt,
    iter,
    ops::Deref,
};

use ed25519_zebra as ed25519;
use multibase::Base;
use serde::{de::Visitor, Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;
use zeroize::Zeroize;

use keystore::{sign, SecretKeyExt};

pub const PUBLICKEYBYTES: usize = std::mem::size_of::<ed25519::VerificationKeyBytes>();
pub use keystore::SecStr;

/// Version of the signature scheme in use
///
/// This is used for future-proofing serialisation. For ergonomics reasons, we
/// avoid introducing single-variant enums just now, and just serialize a
/// version tag alongside the data.
const VERSION: u8 = 0;

pub trait SignError: error::Error + Send + Sync + 'static {}
impl<T: error::Error + Send + Sync + 'static> SignError for T {}

/// A device-specific signing key
#[derive(Clone, Zeroize)]
#[cfg_attr(test, derive(Debug))]
#[zeroize(drop)]
pub struct SecretKey(ed25519::SigningKey);

/// The public part of a `Key``
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct PublicKey(ed25519::VerificationKeyBytes);

impl From<sign::PublicKey> for PublicKey {
    fn from(other: sign::PublicKey) -> PublicKey {
        PublicKey(ed25519::VerificationKeyBytes::from(other.0))
    }
}

impl From<PublicKey> for sign::PublicKey {
    fn from(other: PublicKey) -> Self {
        Self(other.0.into())
    }
}

/// A signature produced by `Key::sign`
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Signature(ed25519::Signature);

impl From<sign::Signature> for Signature {
    fn from(other: sign::Signature) -> Signature {
        Signature(ed25519::Signature::from(other.0))
    }
}

// Key

#[allow(clippy::new_without_default)]
impl SecretKey {
    pub fn new() -> Self {
        let sk = ed25519::SigningKey::new(rand::thread_rng());
        Self(sk)
    }

    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self(ed25519::SigningKey::from(seed))
    }

    pub(crate) fn from_secret(sk: ed25519::SigningKey) -> Self {
        Self(sk)
    }

    pub fn public(&self) -> PublicKey {
        PublicKey(ed25519::VerificationKeyBytes::from(
            ed25519::VerificationKey::from(&self.0),
        ))
    }

    pub fn sign(&self, data: &[u8]) -> Signature {
        Signature(self.0.sign(data))
    }
}

impl fmt::Display for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.public().fmt(f)
    }
}

impl AsRef<[u8]> for SecretKey {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum IntoSecretKeyError {
    #[error("invalid length")]
    InvalidSliceLength,
}

impl SecretKeyExt for SecretKey {
    type Metadata = ();
    type Error = IntoSecretKeyError;

    fn from_bytes_and_meta(bytes: SecStr, _metadata: &Self::Metadata) -> Result<Self, Self::Error> {
        let sk = ed25519::SigningKey::try_from(bytes.unsecure())
            .map_err(|_| IntoSecretKeyError::InvalidSliceLength)?;
        Ok(Self::from_secret(sk))
    }

    fn metadata(&self) -> Self::Metadata {}
}

#[async_trait]
impl sign::Signer for SecretKey {
    type Error = Infallible;

    fn public_key(&self) -> sign::PublicKey {
        sign::Signer::public_key(&self)
    }

    async fn sign(&self, data: &[u8]) -> Result<sign::Signature, Self::Error> {
        sign::Signer::sign(&self, data).await
    }
}

#[async_trait]
impl<'a> sign::Signer for &'a SecretKey {
    type Error = Infallible;

    fn public_key(&self) -> sign::PublicKey {
        sign::PublicKey(ed25519::VerificationKey::from(&self.0).into())
    }

    async fn sign(&self, data: &[u8]) -> Result<sign::Signature, Self::Error> {
        let signature = (*self).sign(data).0;
        Ok(sign::Signature(signature.into()))
    }
}

impl From<SecretKey> for ed25519::SigningKey {
    fn from(key: SecretKey) -> Self {
        key.0
    }
}

// PublicKey

impl PublicKey {
    pub fn verify(&self, sig: &Signature, data: &[u8]) -> bool {
        ed25519::VerificationKey::try_from(self.0)
            .and_then(|vk| vk.verify(&sig.0, data))
            .is_ok()
    }

    pub fn from_slice(bs: &[u8]) -> Option<PublicKey> {
        ed25519::VerificationKeyBytes::try_from(bs)
            .map(PublicKey)
            .ok()
    }
}

impl From<ed25519::VerificationKey> for PublicKey {
    fn from(pk: ed25519::VerificationKey) -> Self {
        Self(pk.into())
    }
}

impl From<SecretKey> for PublicKey {
    fn from(k: SecretKey) -> Self {
        k.public()
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.serialize(f)
    }
}

impl AsRef<[u8]> for PublicKey {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl Deref for PublicKey {
    type Target = ed25519::VerificationKeyBytes;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Serialize for PublicKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        multibase::encode(
            Base::Base32Z,
            iter::once(&VERSION)
                .chain(self.as_ref())
                .cloned()
                .collect::<Vec<u8>>(),
        )
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PublicKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct PublicKeyVisitor;

        impl<'de> Visitor<'de> for PublicKeyVisitor {
            type Value = PublicKey;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a PublicKey, version {}", VERSION)
            }

            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let (_, bytes) = multibase::decode(s).map_err(serde::de::Error::custom)?;
                match bytes.split_first() {
                    // impossible, actually
                    None => Err(serde::de::Error::custom("Empty input")),
                    Some((version, data)) => {
                        if version != &VERSION {
                            return Err(serde::de::Error::custom(format!(
                                "Unknown PublicKey version {}",
                                version
                            )));
                        }

                        ed25519::VerificationKeyBytes::try_from(data)
                            .map(PublicKey)
                            .map_err(serde::de::Error::custom)
                    },
                }
            }
        }

        deserializer.deserialize_str(PublicKeyVisitor)
    }
}

impl minicbor::Encode for PublicKey {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.array(2)?.u8(VERSION)?.bytes(self.0.as_ref())?;
        Ok(())
    }
}

impl<'b> minicbor::Decode<'b> for PublicKey {
    fn decode(d: &mut minicbor::Decoder<'b>) -> Result<Self, minicbor::decode::Error> {
        if Some(2) != d.array()? {
            return Err(minicbor::decode::Error::Message("expected 2-element array"));
        }
        if VERSION != d.u8()? {
            return Err(minicbor::decode::Error::Message("Unknown version"));
        }

        let data = d.bytes()?;
        ed25519::VerificationKeyBytes::try_from(data)
            .map(PublicKey)
            .map_err(|_| minicbor::decode::Error::Message("Invalid length for ed25519 public key"))
    }
}

// Signature

impl Signature {
    pub fn verify(&self, data: &[u8], pk: &PublicKey) -> bool {
        pk.verify(self, data)
    }
}

impl fmt::Display for Signature {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.serialize(f)
    }
}

impl From<Signature> for [u8; 64] {
    fn from(sig: Signature) -> [u8; 64] {
        sig.0.into()
    }
}

impl Deref for Signature {
    type Target = ed25519::Signature;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Serialize for Signature {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let bytes: [u8; 64] = self.0.into();
        let mut input = vec![VERSION];
        input.extend(&bytes[..]);

        multibase::encode(Base::Base32Z, input).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Signature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct SignatureVisitor;

        impl<'de> Visitor<'de> for SignatureVisitor {
            type Value = Signature;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a Signature, version {}", VERSION)
            }

            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let (_, bytes) = multibase::decode(s).map_err(serde::de::Error::custom)?;
                match bytes.split_first() {
                    // impossible, actually
                    None => Err(serde::de::Error::custom("Empty input")),
                    Some((version, data)) => {
                        if version != &VERSION {
                            return Err(serde::de::Error::custom(format!(
                                "Unknown Signature version {}",
                                version
                            )));
                        }

                        ed25519::Signature::try_from(data)
                            .map(Signature)
                            .map_err(serde::de::Error::custom)
                    },
                }
            }
        }

        deserializer.deserialize_str(SignatureVisitor)
    }
}

impl minicbor::Encode for Signature {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        let bytes: [u8; 64] = self.0.into();
        e.array(2)?.u8(VERSION)?.bytes(&bytes)?;
        Ok(())
    }
}

impl<'b> minicbor::Decode<'b> for Signature {
    fn decode(d: &mut minicbor::Decoder<'b>) -> Result<Self, minicbor::decode::Error> {
        if Some(2) != d.array()? {
            return Err(minicbor::decode::Error::Message("expected 2-element array"));
        }
        if VERSION != d.u8()? {
            return Err(minicbor::decode::Error::Message("Unknown version"));
        }

        let data = d.bytes()?;
        ed25519::Signature::try_from(data)
            .map(Signature)
            .map_err(|_| minicbor::decode::Error::Message("Invalid length for ed25519 signature"))
    }
}
