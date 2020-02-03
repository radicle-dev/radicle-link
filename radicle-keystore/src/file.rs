use std::{
    fmt::{self, Debug, Display},
    fs::File,
    io,
    marker::PhantomData,
    path::{Path, PathBuf},
};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::{
    crypto::{self, SealedKey},
    AndMeta,
    IntoSecretKey,
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

#[derive(Debug)]
pub enum Error<P> {
    KeyExists,
    NoSuchKey,
    Crypto(crypto::UnsealError),
    Pinentry(P),
    Serde(serde_cbor::error::Error),
    Io(io::Error),
}

impl<P: Display + Debug> std::error::Error for Error<P> {}

impl<P: Display> Display for Error<P> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::KeyExists => f.write_str("Key exists, refusing to overwrite"),
            Self::NoSuchKey => f.write_str("No key found"),
            Self::Crypto(e) => write!(f, "Error unsealing key: {}", e),
            Self::Pinentry(e) => write!(f, "{}", e),
            Self::Serde(e) => write!(f, "{}", e),
            Self::Io(e) => write!(f, "{}", e),
        }
    }
}

impl<P> From<io::Error> for Error<P> {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl<P> From<serde_cbor::error::Error> for Error<P> {
    fn from(e: serde_cbor::error::Error) -> Self {
        Self::Serde(e)
    }
}

impl<P, PK, SK, M> Storage for FileStorage<P, PK, SK, M>
where
    P: Pinentry,
    P::Error: Display + Debug,
    SK: AsRef<[u8]> + IntoSecretKey<M>,
    PK: Clone + Serialize + DeserializeOwned,
    M: Clone + Serialize + DeserializeOwned,
{
    type Pinentry = P;

    type PublicKey = PK;
    type SecretKey = SK;

    type Metadata = M;

    type Error = Error<P::Error>;

    fn put_keypair(
        &mut self,
        keypair: Keypair<Self::PublicKey, Self::SecretKey>,
        metadata: Self::Metadata,
    ) -> Result<(), Self::Error> {
        if self.key_file_path().exists() {
            return Err(Error::KeyExists);
        }

        let sealed_key = {
            let passphrase = self.pinentry.get_passphrase().map_err(Error::Pinentry)?;
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
    ) -> Result<AndMeta<Keypair<Self::PublicKey, Self::SecretKey>, Self::Metadata>, Self::Error>
    {
        if !self.key_file_path().exists() {
            return Err(Error::NoSuchKey);
        }

        let stored: Stored<Self::PublicKey, Self::Metadata> =
            serde_cbor::from_reader(File::open(self.key_file_path())?)?;
        let passphrase = self.pinentry.get_passphrase().map_err(Error::Pinentry)?;

        let secret_key = stored
            .secret_key
            .unseal(passphrase)
            .map_err(Error::Crypto)
            .map(|sec| Self::SecretKey::into_secret_key(sec, &stored.metadata))?;

        Ok(AndMeta {
            value: Keypair {
                public_key: stored.public_key.clone(),
                secret_key,
            },
            metadata: stored.metadata,
        })
    }

    fn show_key(&self) -> Result<AndMeta<Self::PublicKey, Self::Metadata>, Self::Error> {
        if !self.key_file_path().exists() {
            return Err(Error::NoSuchKey);
        }

        let stored: Stored<Self::PublicKey, Self::Metadata> =
            serde_cbor::from_reader(File::open(self.key_file_path())?)?;

        Ok(AndMeta {
            value: stored.public_key,
            metadata: stored.metadata,
        })
    }
}
