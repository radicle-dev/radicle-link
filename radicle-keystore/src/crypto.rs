use std::fmt::{self, Display};

use secstr::{SecStr, SecUtf8};
use serde::{Deserialize, Serialize};
use sodiumoxide::crypto::{pwhash, secretbox};

use crate::pinentry::Pinentry;

/// Class of types which can seal (encrypt) a secret, and unseal (decrypt) it
/// from it's sealed form.
///
/// It is up to the user to perform conversion from and to domain types.
pub trait Crypto: Sized {
    type SecretBox;
    type Error;

    fn seal<K: AsRef<[u8]>>(&self, secret: K) -> Result<Self::SecretBox, Self::Error>;
    fn unseal(&self, secret_box: Self::SecretBox) -> Result<SecStr, Self::Error>;
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SecretBox {
    nonce: secretbox::Nonce,
    salt: pwhash::Salt,
    sealed: Vec<u8>,
}

#[derive(Debug)]
pub enum SecretBoxError<PinentryError> {
    InvalidKey,
    Pinentry(PinentryError),
}

impl<E: Display> Display for SecretBoxError<E> {
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

/// A [`Crypto`] implementation using `libsodium`'s "secretbox". The encryption
/// key is derived from a passphrase using the primitives provided by
/// `libsodium`'s `pwhash` (hence the name).
///
/// The resulting [`SecretBox`] stores the ciphertext alongside cleartext salt
/// and nonce values.
pub struct Pwhash<P> {
    pinentry: P,
}

impl<P> Pwhash<P> {
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

impl<P: Pinentry> Crypto for Pwhash<P> {
    type SecretBox = SecretBox;
    type Error = SecretBoxError<P::Error>;

    fn seal<K: AsRef<[u8]>>(&self, secret: K) -> Result<Self::SecretBox, Self::Error> {
        let passphrase = self
            .pinentry
            .get_passphrase()
            .map_err(SecretBoxError::Pinentry)?;

        let nonce = secretbox::gen_nonce();
        let salt = pwhash::gen_salt();

        let sealed = secretbox::seal(
            secret.as_ref(),
            &nonce,
            &Self::derive_key(&salt, &passphrase),
        );

        Ok(SecretBox {
            nonce,
            salt,
            sealed,
        })
    }

    fn unseal(&self, secret_box: Self::SecretBox) -> Result<SecStr, Self::Error> {
        let passphrase = self
            .pinentry
            .get_passphrase()
            .map_err(SecretBoxError::Pinentry)?;

        secretbox::open(
            &secret_box.sealed,
            &secret_box.nonce,
            &Self::derive_key(&secret_box.salt, &passphrase),
        )
        .map_err(|()| SecretBoxError::InvalidKey)
        .map(SecStr::new)
    }
}
