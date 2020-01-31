use std::{convert::TryFrom, marker::PhantomData, time::SystemTime};

use failure::format_err;
use secstr::SecUtf8;
use sodiumoxide::crypto::{pwhash, secretbox};

pub(crate) struct SealedKey<PK, SK> {
    nonce: secretbox::Nonce,
    salt: pwhash::Salt,
    pub created_at: u64,
    pub public_key: PK,
    sealed_key: Vec<u8>,

    _marker: PhantomData<SK>,
}

pub(crate) enum Error<E> {
    InvalidPassphrase,
    TryFrom(E),
}

impl<PK, SK> SealedKey<PK, SK> {
    pub fn seal(public_key: PK, secret_key: SK, passphrase: SecUtf8, created_at: SystemTime) -> Self
    where
        SK: AsRef<[u8]>,
    {
        let nonce = secretbox::gen_nonce();
        let salt = pwhash::gen_salt();

        let sealed_key =
            secretbox::seal(secret_key.as_ref(), &nonce, &derive_key(&salt, &passphrase));

        Self {
            nonce,
            salt,
            created_at: created_at
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("System clock went backwards")
                .as_secs(),
            public_key,
            sealed_key,

            _marker: PhantomData,
        }
    }

    pub fn unseal<E>(&self, passphrase: SecUtf8) -> Result<SK, Error<E>>
    where
        SK: TryFrom<Vec<u8>>,
        E: <SK as TryFrom<Vec<u8>>>::Error,
    {
        secretbox::open(
            &self.sealed_key,
            &self.nonce,
            &derive_key(&self.salt, &passphrase),
        )
        .map_err(|_| Error::InvalidPassphrase)
        .and_then(|unsealed| SK::try_from(unsealed).map_err(Error::TryFrom))
    }
}

fn derive_key(salt: &pwhash::Salt, passphrase: &SecUtf8) -> secretbox::Key {
    let mut k = secretbox::Key([0; secretbox::KEYBYTES]);
    let secretbox::Key(ref mut kb) = k;
    pwhash::derive_key_interactive(kb, passphrase.unsecure().as_bytes(), salt)
        .expect("Key derivation failed");
    k
}
