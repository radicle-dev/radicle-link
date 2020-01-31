use std::{
    convert::TryFrom,
    fs::File,
    io,
    marker::PhantomData,
    path::{Path, PathBuf},
};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::{
    crypto::{self, SealedKey},
    AndMeta,
    Keypair,
    Pinentry,
    Storage,
};

pub struct FileStorage<P, PK, SK, M> {
    key_file_path: PathBuf,
    pinentry: P,

    _marker: PhantomData<(PK, SK, M)>,
}

impl<P, PK, SK, M> FileStorage<P, PK, SK, M> {
    pub fn new(key_file_path: &Path, pinentry: P) -> Self {
        Self {
            key_file_path: key_file_path.to_path_buf(),
            pinentry,
            _marker: PhantomData,
        }
    }

    pub fn key_file_path(&self) -> &Path {
        self.key_file_path.as_path()
    }
}

#[derive(Serialize, Deserialize)]
struct Stored<PK, M> {
    public_key: PK,
    secret_key: SealedKey,
    metadata: M,
}

pub enum PutError<P> {
    KeyExists,
    Pinentry(P),
    Serde(serde_cbor::error::Error),
    Io(io::Error),
}

impl<P> From<io::Error> for PutError<P> {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl<P> From<serde_cbor::error::Error> for PutError<P> {
    fn from(e: serde_cbor::error::Error) -> Self {
        Self::Serde(e)
    }
}

pub enum GetError<C, P> {
    NoSuchKey,
    Crypto(crypto::Error<C>),
    Pinentry(P),
    Serde(serde_cbor::error::Error),
    Io(io::Error),
}

impl<C, P> From<io::Error> for GetError<C, P> {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl<C, P> From<serde_cbor::error::Error> for GetError<C, P> {
    fn from(e: serde_cbor::error::Error) -> Self {
        Self::Serde(e)
    }
}

pub enum ShowError {
    NoSuchKey,
    Serde(serde_cbor::error::Error),
    Io(io::Error),
}

impl From<io::Error> for ShowError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_cbor::error::Error> for ShowError {
    fn from(e: serde_cbor::error::Error) -> Self {
        Self::Serde(e)
    }
}

impl<P, PK, SK, M> Storage<P> for FileStorage<P, PK, SK, M>
where
    P: Pinentry,
    SK: AsRef<[u8]> + TryFrom<Vec<u8>>,
    PK: Clone + Serialize + DeserializeOwned,
    M: Clone + Serialize + DeserializeOwned,
{
    type PublicKey = PK;
    type SecretKey = SK;

    type Metadata = M;

    type PutError = PutError<P::Error>;
    type GetError = GetError<<SK as TryFrom<Vec<u8>>>::Error, P::Error>;
    type ShowError = ShowError;

    fn put_keypair(
        &mut self,
        keypair: Keypair<Self::PublicKey, Self::SecretKey>,
        metadata: Self::Metadata,
    ) -> Result<(), Self::PutError> {
        if self.key_file_path().exists() {
            return Err(PutError::KeyExists);
        }

        let sealed_key = {
            let passphrase = self.pinentry.get_passphrase().map_err(PutError::Pinentry)?;
            SealedKey::seal(keypair.secret_key, passphrase)
        };

        let key_file = File::create(self.key_file_path())?;
        serde_cbor::to_writer(
            &key_file,
            &Stored {
                public_key: keypair.public_key,
                secret_key: sealed_key,
                metadata,
            },
        )?;
        key_file.sync_all()?;

        Ok(())
    }

    fn get_keypair(
        &self,
    ) -> Result<AndMeta<Keypair<Self::PublicKey, Self::SecretKey>, Self::Metadata>, Self::GetError>
    {
        if !self.key_file_path().exists() {
            return Err(Self::GetError::NoSuchKey);
        }

        let stored: Stored<Self::PublicKey, Self::Metadata> =
            serde_cbor::from_reader(File::open(self.key_file_path())?)?;
        let passphrase = self
            .pinentry
            .get_passphrase()
            .map_err(Self::GetError::Pinentry)?;

        let secret_key = stored
            .secret_key
            .unseal(passphrase)
            .map_err(Self::GetError::Crypto)?;

        Ok(AndMeta {
            value: Keypair {
                public_key: stored.public_key.clone(),
                secret_key,
            },
            metadata: stored.metadata,
        })
    }

    fn show_key(&self) -> Result<AndMeta<Self::PublicKey, Self::Metadata>, Self::ShowError> {
        if !self.key_file_path().exists() {
            return Err(Self::ShowError::NoSuchKey);
        }

        let stored: Stored<Self::PublicKey, Self::Metadata> =
            serde_cbor::from_reader(File::open(self.key_file_path())?)?;

        Ok(AndMeta {
            value: stored.public_key,
            metadata: stored.metadata,
        })
    }
}
