use std::fmt;

use secstr::{SecStr, SecUtf8};
use serde::{Deserialize, Serialize};
use sodiumoxide::crypto::{pwhash, secretbox};

#[derive(Serialize, Deserialize)]
pub(crate) struct SealedKey {
    nonce: secretbox::Nonce,
    salt: pwhash::Salt,
    sealed: Vec<u8>,
}

#[derive(Debug)]
pub struct UnsealError;

impl fmt::Display for UnsealError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("Invalid passphrase")
    }
}

impl SealedKey {
    pub fn seal<K>(secret_key: K, passphrase: SecUtf8) -> Self
    where
        K: AsRef<[u8]>,
    {
        let nonce = secretbox::gen_nonce();
        let salt = pwhash::gen_salt();

        let sealed = secretbox::seal(secret_key.as_ref(), &nonce, &derive_key(&salt, &passphrase));

        Self {
            nonce,
            salt,
            sealed,
        }
    }

    pub fn unseal(&self, passphrase: SecUtf8) -> Result<SecStr, UnsealError> {
        secretbox::open(
            &self.sealed,
            &self.nonce,
            &derive_key(&self.salt, &passphrase),
        )
        .map_err(|()| UnsealError)
        .map(SecStr::new)
    }
}

fn derive_key(salt: &pwhash::Salt, passphrase: &SecUtf8) -> secretbox::Key {
    let mut k = secretbox::Key([0; secretbox::KEYBYTES]);
    let secretbox::Key(ref mut kb) = k;
    pwhash::derive_key_interactive(kb, passphrase.unsecure().as_bytes(), salt)
        .expect("Key derivation failed");
    k
}
