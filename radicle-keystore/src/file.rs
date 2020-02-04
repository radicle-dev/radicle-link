use std::{
    fmt::{self, Debug, Display},
    fs::File,
    io,
    marker::PhantomData,
    path::{Path, PathBuf},
};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::{crypto::Crypto, Keypair, Keystore, SecretKeyExt};

pub struct FileStorage<C, PK, SK, M> {
    key_file_path: PathBuf,
    crypto: C,

    _marker: PhantomData<(PK, SK, M)>,
}

impl<C, PK, SK, M> FileStorage<C, PK, SK, M> {
    pub fn new(key_file_path: &Path, crypto: C) -> Self {
        Self {
            key_file_path: key_file_path.to_path_buf(),
            crypto,

            _marker: PhantomData,
        }
    }

    pub fn key_file_path(&self) -> &Path {
        self.key_file_path.as_path()
    }
}

#[derive(Serialize, Deserialize)]
struct Stored<PK, S, M> {
    public_key: PK,
    secret_key: S,
    metadata: M,
}

#[derive(Debug)]
pub enum Error<Crypto, Conversion> {
    KeyExists,
    NoSuchKey,
    Crypto(Crypto),
    Conversion(Conversion),
    Serde(serde_cbor::error::Error),
    Io(io::Error),
}

impl<Crypto, Conversion> std::error::Error for Error<Crypto, Conversion>
where
    Crypto: Display + Debug,
    Conversion: Display + Debug,
{
}

impl<Crypto, Conversion> Display for Error<Crypto, Conversion>
where
    Crypto: Display + Debug,
    Conversion: Display + Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::KeyExists => f.write_str("Key exists, refusing to overwrite"),
            Self::NoSuchKey => f.write_str("No key found"),
            Self::Conversion(e) => write!(f, "Error reconstructing sealed key: {}", e),
            Self::Crypto(e) => write!(f, "Error unsealing key: {}", e),
            Self::Serde(e) => write!(f, "{}", e),
            Self::Io(e) => write!(f, "{}", e),
        }
    }
}

impl<Crypto, Conversion> From<io::Error> for Error<Crypto, Conversion> {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl<Crypto, Conversion> From<serde_cbor::error::Error> for Error<Crypto, Conversion> {
    fn from(e: serde_cbor::error::Error) -> Self {
        Self::Serde(e)
    }
}

impl<C, PK, SK, M> Keystore for FileStorage<C, PK, SK, M>
where
    C: Crypto,
    C::Error: Display + Debug,
    C::SecretBox: Serialize + DeserializeOwned,

    SK: AsRef<[u8]> + SecretKeyExt<Metadata = M>,
    <SK as SecretKeyExt>::Error: Display + Debug,

    PK: Clone + From<SK> + Serialize + DeserializeOwned,
    M: Clone + Serialize + DeserializeOwned,
{
    type PublicKey = PK;
    type SecretKey = SK;
    type Metadata = M;
    type Error = Error<C::Error, <SK as SecretKeyExt>::Error>;

    fn put_key(&mut self, key: Self::SecretKey) -> Result<(), Self::Error> {
        if self.key_file_path().exists() {
            return Err(Error::KeyExists);
        }

        let metadata = key.metadata();
        let sealed_key = self.crypto.seal(&key).map_err(Error::Crypto)?;

        let key_file = File::create(self.key_file_path())?;
        serde_cbor::to_writer(
            &key_file,
            &Stored {
                public_key: Self::PublicKey::from(key),
                secret_key: sealed_key,
                metadata,
            },
        )?;
        key_file.sync_all()?;

        Ok(())
    }

    fn get_key(&self) -> Result<Keypair<Self::PublicKey, Self::SecretKey>, Self::Error> {
        if !self.key_file_path().exists() {
            return Err(Error::NoSuchKey);
        }

        let stored: Stored<Self::PublicKey, <C as Crypto>::SecretBox, Self::Metadata> =
            serde_cbor::from_reader(File::open(self.key_file_path())?)?;

        let secret_key = {
            let sbox = stored.secret_key;
            let meta = stored.metadata;

            self.crypto
                .unseal(sbox)
                .map_err(Error::Crypto)
                .and_then(|sec| {
                    Self::SecretKey::from_bytes_and_meta(sec, &meta).map_err(Error::Conversion)
                })
        }?;

        Ok(Keypair {
            public_key: stored.public_key,
            secret_key,
        })
    }

    fn show_key(&self) -> Result<(Self::PublicKey, Self::Metadata), Self::Error> {
        if !self.key_file_path().exists() {
            return Err(Error::NoSuchKey);
        }

        let stored: Stored<Self::PublicKey, <C as Crypto>::SecretBox, Self::Metadata> =
            serde_cbor::from_reader(File::open(self.key_file_path())?)?;

        Ok((stored.public_key, stored.metadata))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        crypto::{Passphrase, PassphraseError},
        pinentry::Pinentry,
        test::*,
    };
    use tempfile::tempdir;

    fn with_fs_store<F, P>(pin: P, f: F)
    where
        F: FnOnce(FileStorage<Passphrase<P>, PublicKey, SecretKey, ()>) -> (),
        P: Pinentry,
    {
        let tmp = tempdir().expect("Can't get tempdir");
        f(FileStorage::new(
            &tmp.path().join("test.key"),
            Passphrase::new(pin),
        ))
    }

    #[test]
    fn test_get_after_put() {
        with_fs_store(default_passphrase(), get_after_put)
    }

    #[test]
    fn test_put_twice() {
        with_fs_store(default_passphrase(), |store| {
            put_twice(store, Error::KeyExists)
        })
    }

    #[test]
    fn test_get_empty() {
        with_fs_store(default_passphrase(), |store| {
            get_empty(store, Error::NoSuchKey)
        })
    }

    #[test]
    fn test_passphrase_mismatch() {
        with_fs_store(PinCycle::new(&["right".into(), "wrong".into()]), |store| {
            passphrase_mismatch(store, Error::Crypto(PassphraseError::InvalidKey))
        })
    }
}
