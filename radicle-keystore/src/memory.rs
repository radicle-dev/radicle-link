use std::{
    fmt::{self, Debug, Display},
    marker::PhantomData,
};

use crate::{
    crypto::{self, SealedKey},
    HasMetadata,
    IntoSecretKey,
    Keypair,
    Pinentry,
    Storage,
};

struct Stored<PK, SK, M> {
    public_key: PK,
    secret_key: SealedKey,
    metadata: M,

    _marker: PhantomData<SK>,
}

pub struct MemoryStorage<P, PK, SK, M> {
    key: Option<Stored<PK, SK, M>>,
    pinentry: P,
}

impl<P, PK, SK, M> MemoryStorage<P, PK, SK, M> {
    pub fn new(pinentry: P) -> Self {
        Self {
            key: None,
            pinentry,
        }
    }
}

#[derive(Debug)]
pub enum Error<Pinentry, Conversion> {
    KeyExists,
    NoSuchKey,
    Conversion(Conversion),
    Crypto(crypto::UnsealError),
    Pinentry(Pinentry),
}

impl<P, C> std::error::Error for Error<P, C>
where
    P: Display + Debug,
    C: Display + Debug,
{
}

impl<P, C> Display for Error<P, C>
where
    P: Display,
    C: Display,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::KeyExists => f.write_str("Key exists, refusing to overwrite"),
            Self::NoSuchKey => f.write_str("No key found"),
            Self::Conversion(e) => write!(f, "Error reconstructing sealed key: {}", e),
            Self::Crypto(e) => write!(f, "Error unsealing key: {}", e),
            Self::Pinentry(e) => write!(f, "{}", e),
        }
    }
}

impl<P, PK, SK, M> Storage for MemoryStorage<P, PK, SK, M>
where
    P: Pinentry,
    P::Error: Display + Debug,

    SK: AsRef<[u8]> + IntoSecretKey<Metadata = M> + HasMetadata<Metadata = M>,
    <SK as IntoSecretKey>::Error: Display + Debug,

    PK: Clone + From<SK>,
    M: Clone,
{
    type Pinentry = P;

    type PublicKey = PK;
    type SecretKey = SK;

    type Metadata = M;

    type Error = Error<P::Error, <SK as IntoSecretKey>::Error>;

    fn put_key(&mut self, key: Self::SecretKey) -> Result<(), Self::Error> {
        if self.key.is_some() {
            return Err(Error::KeyExists);
        }

        let metadata = key.metadata();
        let passphrase = self.pinentry.get_passphrase().map_err(Error::Pinentry)?;
        let sealed_key = SealedKey::seal(&key, passphrase);
        self.key = Some(Stored {
            public_key: Self::PublicKey::from(key),
            secret_key: sealed_key,
            metadata,

            _marker: PhantomData,
        });

        Ok(())
    }

    fn get_key(&self) -> Result<Keypair<Self::PublicKey, Self::SecretKey>, Self::Error> {
        match &self.key {
            None => Err(Error::NoSuchKey),
            Some(stored) => {
                let passphrase = self.pinentry.get_passphrase().map_err(Error::Pinentry)?;
                let sk = stored
                    .secret_key
                    .unseal(passphrase)
                    .map_err(Error::Crypto)
                    .and_then(|sec| {
                        Self::SecretKey::into_secret_key(sec, &stored.metadata)
                            .map_err(Error::Conversion)
                    })?;

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
