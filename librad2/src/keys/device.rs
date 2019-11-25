use std::fmt;

use bs58;
use sodiumoxide::crypto::sign;
use time;

use crate::keys::pgp;

/// A device-specific signing key
#[derive(Clone, Eq, PartialEq)]
pub struct Key {
    sk: sign::SecretKey,
    pub created_at: i64,
}

/// The public part of a `Key``
pub struct PublicKey(sign::PublicKey);

/// A signature produced by `Key::sign`
pub struct Signature(sign::Signature);

// Key

impl Key {
    pub fn new() -> Self {
        let (_, sk) = sign::gen_keypair();
        let created_at = time::now_utc().to_timespec().sec;
        Key { sk, created_at }
    }

    #[cfg(test)]
    pub fn from_seed(seed: &sign::Seed, created_at: time::Tm) -> Self {
        let (_, sk) = sign::keypair_from_seed(seed);
        Key {
            sk,
            created_at: created_at.to_timespec().sec,
        }
    }

    pub(crate) fn from_secret(sk: sign::SecretKey, created_at: time::Tm) -> Self {
        Key {
            sk,
            created_at: created_at.to_timespec().sec,
        }
    }

    pub fn public_key(&self) -> PublicKey {
        PublicKey(self.sk.public_key())
    }

    pub fn sign(&self, data: &[u8]) -> Signature {
        Signature(sign::sign_detached(data, &self.sk))
    }

    pub fn as_pgp(&self, nickname: &str) -> Result<pgp::Key, pgp::Error> {
        let uid = pgp::UserID::from_address(None, None, format!("{}@{}", nickname, self))
            .expect("messed up UserID");
        pgp::Key::from_sodium(
            &self.sk,
            uid,
            time::at(time::Timespec::new(self.created_at, 0)),
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
        self.public_key().fmt(f)
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
        sign::verify_detached(&sig.0, &data, &self.0)
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            bs58::encode(self.0)
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

// Signature

impl Signature {
    pub fn verify(&self, data: &[u8], pk: &PublicKey) -> bool {
        sign::verify_detached(&self.0, &data, &pk.0)
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
        assert!(sig.verify(&DATA_TO_SIGN, &key.public_key()))
    }

    #[test]
    fn test_sign_verify_via_pubkey() {
        let key = Key::new();
        let sig = key.sign(&DATA_TO_SIGN);
        assert!(key.public_key().verify(&sig, &DATA_TO_SIGN))
    }
}
