use std::convert::TryFrom;

use secstr::SecUtf8;
use serde::{Deserialize, Serialize};
use sodiumoxide::crypto::{pwhash, secretbox};

#[derive(Serialize, Deserialize)]
pub(crate) struct SealedKey {
    nonce: secretbox::Nonce,
    salt: pwhash::Salt,
    sealed: Vec<u8>,
}

pub enum Error<E> {
    InvalidPassphrase,
    TryFrom(E),
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

    pub fn unseal<K>(&self, passphrase: SecUtf8) -> Result<K, Error<<K as TryFrom<Vec<u8>>>::Error>>
    where
        K: TryFrom<Vec<u8>>,
    {
        secretbox::open(
            &self.sealed,
            &self.nonce,
            &derive_key(&self.salt, &passphrase),
        )
        .map_err(|()| Error::InvalidPassphrase)
        .and_then(|unsealed| K::try_from(unsealed).map_err(Error::TryFrom))
    }
}

fn derive_key(salt: &pwhash::Salt, passphrase: &SecUtf8) -> secretbox::Key {
    let mut k = secretbox::Key([0; secretbox::KEYBYTES]);
    let secretbox::Key(ref mut kb) = k;
    pwhash::derive_key_interactive(kb, passphrase.unsecure().as_bytes(), salt)
        .expect("Key derivation failed");
    k
}
