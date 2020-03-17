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
    fmt::{self, Debug, Display},
    marker::PhantomData,
};

use crate::{crypto::Crypto, Keypair, Keystore, SecretKeyExt};

struct Stored<PK, S, M> {
    public_key: PK,
    secret_key: S,
    metadata: M,
}

/// [`Keystore`] implementation which stores the encrypted key in memory.
///
/// This is provided mainly for testing in environments where hitting the
/// filesystem is undesirable, and otherwise equivalent to [`FileStorage`].
///
/// [`FileStorage`]: ../struct.FileStorage.html
pub struct MemoryStorage<C: Crypto, PK, SK, M> {
    key: Option<Stored<PK, C::SecretBox, M>>,
    crypto: C,

    _marker: PhantomData<SK>,
}

impl<C: Crypto, PK, SK, M> MemoryStorage<C, PK, SK, M> {
    pub fn new(crypto: C) -> Self {
        Self {
            key: None,
            crypto,

            _marker: PhantomData,
        }
    }
}

#[derive(Debug)]
pub enum Error<Crypto, Conversion> {
    KeyExists,
    NoSuchKey,
    Crypto(Crypto),
    Conversion(Conversion),
}

impl<Crypto, Conversion> std::error::Error for Error<Crypto, Conversion>
where
    Crypto: Display + Debug,
    Conversion: Display + Debug,
{
}

impl<Crypto, Conversion> Display for Error<Crypto, Conversion>
where
    Crypto: Display,
    Conversion: Display,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::KeyExists => f.write_str("Key exists, refusing to overwrite"),
            Self::NoSuchKey => f.write_str("No key found"),
            Self::Conversion(e) => write!(f, "Error reconstructing sealed key: {}", e),
            Self::Crypto(e) => write!(f, "Error unsealing key: {}", e),
        }
    }
}

impl<C, PK, SK, M> Keystore for MemoryStorage<C, PK, SK, M>
where
    C: Crypto,
    C::Error: Display + Debug,
    C::SecretBox: Clone,

    SK: AsRef<[u8]> + SecretKeyExt<Metadata = M>,
    <SK as SecretKeyExt>::Error: Display + Debug,

    PK: Clone + From<SK>,
    M: Clone,
{
    type PublicKey = PK;
    type SecretKey = SK;
    type Metadata = M;
    type Error = Error<C::Error, <SK as SecretKeyExt>::Error>;

    fn put_key(&mut self, key: Self::SecretKey) -> Result<(), Self::Error> {
        if self.key.is_some() {
            return Err(Error::KeyExists);
        }

        let metadata = key.metadata();
        let sealed_key = self.crypto.seal(&key).map_err(Error::Crypto)?;
        self.key = Some(Stored {
            public_key: Self::PublicKey::from(key),
            secret_key: sealed_key,
            metadata,
        });

        Ok(())
    }

    fn get_key(&self) -> Result<Keypair<Self::PublicKey, Self::SecretKey>, Self::Error> {
        match &self.key {
            None => Err(Error::NoSuchKey),
            Some(ref stored) => {
                let sk = {
                    let sbox = stored.secret_key.clone();
                    let meta = stored.metadata.clone();

                    self.crypto
                        .unseal(sbox)
                        .map_err(Error::Crypto)
                        .and_then(|sec| {
                            Self::SecretKey::from_bytes_and_meta(sec, &meta)
                                .map_err(Error::Conversion)
                        })
                }?;

                Ok(Keypair {
                    public_key: stored.public_key.clone(),
                    secret_key: sk,
                })
            },
        }
    }

    fn show_key(&self) -> Result<(Self::PublicKey, Self::Metadata), Self::Error> {
        self.key
            .as_ref()
            .ok_or(Error::NoSuchKey)
            .map(|sealed| (sealed.public_key.clone(), sealed.metadata.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        crypto::{Pwhash, SecretBoxError},
        pinentry::Pinentry,
        test::*,
    };

    fn with_mem_store<F, P>(pin: P, f: F)
    where
        F: FnOnce(MemoryStorage<Pwhash<P>, PublicKey, SecretKey, ()>) -> (),
        P: Pinentry,
    {
        f(MemoryStorage::new(Pwhash::new(pin)))
    }

    #[test]
    fn test_get_after_put() {
        with_mem_store(default_passphrase(), get_after_put)
    }

    #[test]
    fn test_put_twice() {
        with_mem_store(default_passphrase(), |store| {
            put_twice(store, Error::KeyExists)
        })
    }

    #[test]
    fn test_get_empty() {
        with_mem_store(default_passphrase(), |store| {
            get_empty(store, Error::NoSuchKey)
        })
    }

    #[test]
    fn test_passphrase_mismatch() {
        with_mem_store(PinCycle::new(&["right".into(), "wrong".into()]), |store| {
            passphrase_mismatch(store, Error::Crypto(SecretBoxError::InvalidKey))
        })
    }
}
