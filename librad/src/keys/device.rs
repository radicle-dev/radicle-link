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
    fmt::{self, Display},
    ops::Deref,
    time::SystemTime,
};

use ::pgp::conversions::Time;
use bit_vec::BitVec;
use secstr::SecStr;
use serde::{Deserialize, Serialize};
use sodiumoxide::crypto::sign::ed25519;

use keystore::SecretKeyExt;

use crate::keys::pgp;

pub use ed25519::PUBLICKEYBYTES;

/// A device-specific signing key
#[derive(Clone, Eq, PartialEq)]
pub struct Key {
    sk: ed25519::SecretKey,
    /// Time this key was created, normalised seconds precision.
    created_at: SystemTime,
}

/// The public part of a `Key``
#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PublicKey(ed25519::PublicKey);

/// A signature produced by `Key::sign`
#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub struct Signature(ed25519::Signature);

// Key

impl Key {
    pub fn new() -> Self {
        let (_, sk) = ed25519::gen_keypair();
        let created_at = SystemTime::now().canonicalize();
        Key { sk, created_at }
    }

    #[cfg(test)]
    pub fn from_seed(seed: &ed25519::Seed, created_at: SystemTime) -> Self {
        let (_, sk) = ed25519::keypair_from_seed(seed);
        Key {
            sk,
            created_at: created_at.canonicalize(),
        }
    }

    pub(crate) fn from_secret(sk: ed25519::SecretKey, created_at: SystemTime) -> Self {
        Key {
            sk,
            created_at: created_at.canonicalize(),
        }
    }

    pub fn public(&self) -> PublicKey {
        PublicKey(self.sk.public_key())
    }

    pub fn created_at(&self) -> SystemTime {
        self.created_at
    }

    pub fn sign(&self, data: &[u8]) -> Signature {
        Signature(ed25519::sign_detached(data, &self.sk))
    }

    pub fn into_pgp(
        self,
        nickname: &str,
        fullname: Option<String>,
    ) -> Result<pgp::Key, pgp::Error> {
        let uid = pgp::UserID::from_address(fullname, None, format!("{}@{}", nickname, self))
            .expect("messed up UserID");
        pgp::Key::from_sodium(&self.sk, uid, self.created_at)
    }

    const PKCS_ED25519_OID: &'static [u64] = &[1, 3, 101, 112];

    /// Export in PKCS#8 format.
    ///
    /// **NOTE**: this will export private key material. Use with caution.
    ///
    /// Attribution: this code is stolen from the `thrussh` project.
    pub fn as_pkcs8(&self) -> Vec<u8> {
        yasna::construct_der(|writer| {
            writer.write_sequence(|writer| {
                writer.next().write_u32(1);
                writer.next().write_sequence(|writer| {
                    writer
                        .next()
                        .write_oid(&yasna::models::ObjectIdentifier::from_slice(
                            Self::PKCS_ED25519_OID,
                        ));
                });
                let seed = yasna::construct_der(|writer| writer.write_bytes(&self.sk[..32]));
                writer.next().write_bytes(&seed);
                writer
                    .next()
                    .write_tagged(yasna::Tag::context(1), |writer| {
                        let public = &self.sk[32..];
                        writer.write_bitvec(&BitVec::from_bytes(&public))
                    })
            })
        })
    }
}

impl Default for Key {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.public().fmt(f)
    }
}

impl AsRef<[u8]> for Key {
    fn as_ref(&self) -> &[u8] {
        self.sk.as_ref()
    }
}

#[derive(Debug)]
pub enum IntoSecretKeyError {
    InvalidSliceLength,
}

impl Display for IntoSecretKeyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InvalidSliceLength => f.write_str("Invalid slice length"),
        }
    }
}

impl SecretKeyExt for Key {
    type Metadata = SystemTime;
    type Error = IntoSecretKeyError;

    fn from_bytes_and_meta(bytes: SecStr, metadata: &Self::Metadata) -> Result<Self, Self::Error> {
        let sk = ed25519::SecretKey::from_slice(bytes.unsecure())
            .ok_or(IntoSecretKeyError::InvalidSliceLength)?;
        Ok(Self::from_secret(sk, *metadata))
    }

    fn metadata(&self) -> Self::Metadata {
        self.created_at
    }
}

// PublicKey

impl PublicKey {
    pub fn verify(&self, sig: &Signature, data: &[u8]) -> bool {
        ed25519::verify_detached(sig, &data, self)
    }

    pub fn from_slice(bs: &[u8]) -> Option<PublicKey> {
        ed25519::PublicKey::from_slice(&bs).map(PublicKey)
    }

    pub fn from_bs58(s: &str) -> Option<Self> {
        let bytes = match bs58::decode(s.as_bytes())
            .with_alphabet(bs58::alphabet::BITCOIN)
            .into_vec()
        {
            Ok(v) => v,
            Err(_) => return None,
        };
        ed25519::PublicKey::from_slice(&bytes).map(PublicKey)
    }

    pub fn to_bs58(&self) -> String {
        bs58::encode(self.0)
            .with_alphabet(bs58::alphabet::BITCOIN)
            .into_string()
    }
}

impl From<ed25519::PublicKey> for PublicKey {
    fn from(pk: ed25519::PublicKey) -> Self {
        Self(pk)
    }
}

impl From<Key> for PublicKey {
    fn from(k: Key) -> Self {
        k.public()
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.as_ref()))
    }
}

impl AsRef<[u8]> for PublicKey {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl Deref for PublicKey {
    type Target = ed25519::PublicKey;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// Signature

pub mod signature {
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum DecodeError {
        #[error("Could not decode hex string")]
        InvalidHex(#[from] hex::FromHexError),

        #[error("Expected signature to be 64 bytes, got: {actual}")]
        InvalidLength { actual: usize },
    }
}

impl Signature {
    pub fn verify(&self, data: &[u8], pk: &PublicKey) -> bool {
        ed25519::verify_detached(self, &data, pk)
    }

    pub fn from_hex_string(s: &str) -> Result<Self, signature::DecodeError> {
        let bytes = hex::decode(s)?;
        let buffer = if bytes.len() == 64 {
            let mut buffer = [0u8; 64];
            for (i, v) in bytes.iter().enumerate() {
                buffer[i] = *v;
            }
            buffer
        } else {
            return Err(signature::DecodeError::InvalidLength {
                actual: bytes.len(),
            });
        };
        Ok(Self(ed25519::Signature(buffer)))
    }

    pub fn from_bs58(s: &str) -> Option<Self> {
        let bytes = match bs58::decode(s.as_bytes())
            .with_alphabet(bs58::alphabet::BITCOIN)
            .into_vec()
        {
            Ok(v) => v,
            Err(_) => return None,
        };
        let buffer = if bytes.len() == ed25519::SIGNATUREBYTES {
            let mut buffer = [0u8; ed25519::SIGNATUREBYTES];
            for (i, v) in bytes.iter().enumerate() {
                buffer[i] = *v;
            }
            buffer
        } else {
            return None;
        };
        Some(Self(sodiumoxide::crypto::sign::Signature(buffer)))
    }

    pub fn to_bs58(&self) -> String {
        bs58::encode(self.0)
            .with_alphabet(bs58::alphabet::BITCOIN)
            .into_string()
    }
}

impl AsRef<[u8]> for Signature {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl Deref for Signature {
    type Target = ed25519::Signature;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    const DATA_TO_SIGN: &[u8] = b"alors monsieur";

    #[test]
    fn test_sign_verify_via_signature() {
        let key = Key::new();
        let sig = key.sign(&DATA_TO_SIGN);
        assert!(sig.verify(&DATA_TO_SIGN, &key.public()))
    }

    #[test]
    fn test_sign_verify_via_pubkey() {
        let key = Key::new();
        let sig = key.sign(&DATA_TO_SIGN);
        assert!(key.public().verify(&sig, &DATA_TO_SIGN))
    }
}
