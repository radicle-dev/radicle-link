use std::fmt::{self, Display};

use secstr::{SecStr, SecUtf8};
use serde::{Deserialize, Serialize};
use sodiumoxide::crypto::{pwhash, secretbox};

use crate::pinentry::Pinentry;

pub trait Crypto: Sized {
    type SecretBox;
    type Error;

    fn seal<K: AsRef<[u8]>>(&self, secret: K) -> Result<Self::SecretBox, Self::Error>;
    fn unseal(&self, secret_box: Self::SecretBox) -> Result<SecStr, Self::Error>;
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PassphraseBox {
    nonce: secretbox::Nonce,
    salt: pwhash::Salt,
    sealed: Vec<u8>,
}

#[derive(Debug)]
pub enum PassphraseError<PinentryError> {
    InvalidKey,
    Pinentry(PinentryError),
}

impl<E: Display> Display for PassphraseError<E> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InvalidKey => f.write_str(
                "Unable to decrypt secret box using the derived key. \
                Perhaps the passphrase was wrong?",
            ),
            Self::Pinentry(e) => write!(f, "Error getting passphrase: {}", e),
        }
    }
}

pub struct Passphrase<P> {
    pinentry: P,
}

impl<P> Passphrase<P> {
    pub fn new(pinentry: P) -> Self {
        Self { pinentry }
    }

    fn derive_key(salt: &pwhash::Salt, passphrase: &SecUtf8) -> secretbox::Key {
        let mut k = secretbox::Key([0; secretbox::KEYBYTES]);
        let secretbox::Key(ref mut kb) = k;
        pwhash::derive_key_interactive(kb, passphrase.unsecure().as_bytes(), salt)
            .expect("Key derivation failed"); // OOM
        k
    }
}

impl<P: Pinentry> Crypto for Passphrase<P> {
    type SecretBox = PassphraseBox;
    type Error = PassphraseError<P::Error>;

    fn seal<K: AsRef<[u8]>>(&self, secret: K) -> Result<Self::SecretBox, Self::Error> {
        let passphrase = self
            .pinentry
            .get_passphrase()
            .map_err(PassphraseError::Pinentry)?;

        let nonce = secretbox::gen_nonce();
        let salt = pwhash::gen_salt();

        let sealed = secretbox::seal(
            secret.as_ref(),
            &nonce,
            &Self::derive_key(&salt, &passphrase),
        );

        Ok(PassphraseBox {
            nonce,
            salt,
            sealed,
        })
    }

    fn unseal(&self, secret_box: Self::SecretBox) -> Result<SecStr, Self::Error> {
        let passphrase = self
            .pinentry
            .get_passphrase()
            .map_err(PassphraseError::Pinentry)?;

        secretbox::open(
            &secret_box.sealed,
            &secret_box.nonce,
            &Self::derive_key(&secret_box.salt, &passphrase),
        )
        .map_err(|()| PassphraseError::InvalidKey)
        .map(SecStr::new)
    }
}

#[derive(Serialize, Deserialize)]
pub struct ExternalKeyBox {
    nonce: secretbox::Nonce,
    sealed: Vec<u8>,
}

#[derive(Debug)]
pub enum ExternalKeyError {
    InvalidKey,
}

impl Display for ExternalKeyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InvalidKey => f.write_str("Unable to decrypt secret box using the given key."),
        }
    }
}

pub struct ExternalKey {
    key: secretbox::Key, // TODO: make pluggable
}

impl Crypto for ExternalKey {
    type SecretBox = ExternalKeyBox;
    type Error = ExternalKeyError;

    fn seal<K: AsRef<[u8]>>(&self, secret: K) -> Result<Self::SecretBox, Self::Error> {
        let nonce = secretbox::gen_nonce();
        let sealed = secretbox::seal(secret.as_ref(), &nonce, &self.key);
        Ok(ExternalKeyBox { nonce, sealed })
    }

    fn unseal(&self, secret_box: Self::SecretBox) -> Result<SecStr, Self::Error> {
        secretbox::open(&secret_box.sealed, &secret_box.nonce, &self.key)
            .map_err(|()| ExternalKeyError::InvalidKey)
            .map(SecStr::new)
    }
}
