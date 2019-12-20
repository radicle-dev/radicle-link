use std::fmt;
use std::ops::Deref;
use std::time::{Duration, SystemTime};

use bs58;
use sodiumoxide::crypto::sign;

use crate::keys::pgp;

/// A device-specific signing key
#[derive(Clone, Eq, PartialEq)]
pub struct Key {
    sk: sign::SecretKey,
    /// Time since `SystemTime::UNIX_EPOCH`, in seconds.
    pub created_at: u64,
}

/// The public part of a `Key``
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicKey(sign::PublicKey);

/// A signature produced by `Key::sign`
pub struct Signature(sign::Signature);

// Key

impl Key {
    pub fn new() -> Self {
        let (_, sk) = sign::gen_keypair();
        let created_at = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("SystemTime before UNIX_EPOCH. You're screwed.")
            .as_secs();
        Key { sk, created_at }
    }

    #[cfg(test)]
    pub fn from_seed(seed: &sign::Seed, created_at: u64) -> Self {
        let (_, sk) = sign::keypair_from_seed(seed);
        Key { sk, created_at }
    }

    pub(crate) fn from_secret(sk: sign::SecretKey, created_at: u64) -> Self {
        Key { sk, created_at }
    }

    pub fn public(&self) -> PublicKey {
        PublicKey(self.sk.public_key())
    }

    pub fn sign(&self, data: &[u8]) -> Signature {
        Signature(sign::sign_detached(data, &self.sk))
    }

    pub fn into_pgp(
        self,
        nickname: &str,
        fullname: Option<String>,
    ) -> Result<pgp::Key, pgp::Error> {
        let uid = pgp::UserID::from_address(fullname, None, format!("{}@{}", nickname, self))
            .expect("messed up UserID");
        pgp::Key::from_sodium(
            &self.sk,
            uid,
            SystemTime::UNIX_EPOCH
                .checked_add(Duration::from_secs(self.created_at))
                .expect("SystemTime overflow o.O"),
        )
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

// PublicKey

impl PublicKey {
    pub fn verify(&self, sig: &Signature, data: &[u8]) -> bool {
        sign::verify_detached(sig, &data, self)
    }

    pub fn from_slice(bs: &[u8]) -> Option<PublicKey> {
        sign::PublicKey::from_slice(&bs).map(PublicKey)
    }
}

impl From<sign::PublicKey> for PublicKey {
    fn from(pk: sign::PublicKey) -> Self {
        Self(pk)
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            bs58::encode(self)
                .with_alphabet(bs58::alphabet::BITCOIN)
                .into_string()
        )
    }
}

impl AsRef<[u8]> for PublicKey {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl Deref for PublicKey {
    type Target = sign::PublicKey;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// Signature

impl Signature {
    pub fn verify(&self, data: &[u8], pk: &PublicKey) -> bool {
        sign::verify_detached(self, &data, pk)
    }
}

impl AsRef<[u8]> for Signature {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl Deref for Signature {
    type Target = sign::Signature;

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
