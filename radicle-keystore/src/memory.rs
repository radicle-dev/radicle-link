use std::{
    fmt::{self, Debug, Display},
    marker::PhantomData,
};

use crate::{
    crypto::{self, SealedKey},
    AndMeta,
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
pub enum Error<P> {
    KeyExists,
    NoSuchKey,
    Crypto(crypto::UnsealError),
    Pinentry(P),
}

impl<P> std::error::Error for Error<P> where P: Display + Debug {}

impl<P> Display for Error<P>
where
    P: Display,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::KeyExists => f.write_str("Key exists, refusing to overwrite"),
            Self::NoSuchKey => f.write_str("No key found"),
            Self::Crypto(e) => write!(f, "Error unsealing key: {}", e),
            Self::Pinentry(e) => write!(f, "{}", e),
        }
    }
}

impl<P, PK, SK, M> Storage for MemoryStorage<P, PK, SK, M>
where
    P: Pinentry,
    P::Error: Display + Debug,
    SK: AsRef<[u8]> + IntoSecretKey<M>,
    PK: Clone,
    M: Clone,
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
        if self.key.is_some() {
            return Err(Error::KeyExists);
        }

        let passphrase = self.pinentry.get_passphrase().map_err(Error::Pinentry)?;
        let sealed_key = SealedKey::seal(keypair.secret_key, passphrase);
        self.key = Some(Stored {
            public_key: keypair.public_key,
            secret_key: sealed_key,
            metadata,

            _marker: PhantomData,
        });

        Ok(())
    }

    fn get_keypair(
        &self,
    ) -> Result<AndMeta<Keypair<Self::PublicKey, Self::SecretKey>, Self::Metadata>, Self::Error>
    {
        match &self.key {
            None => Err(Error::NoSuchKey),
            Some(stored) => {
                let passphrase = self.pinentry.get_passphrase().map_err(Error::Pinentry)?;
                let sk = stored
                    .secret_key
                    .unseal(passphrase)
                    .map_err(Error::Crypto)
                    .map(|sec| Self::SecretKey::into_secret_key(sec, &stored.metadata))?;

                Ok(AndMeta {
                    value: Keypair {
                        public_key: stored.public_key.clone(),
                        secret_key: sk,
                    },
                    metadata: stored.metadata.clone(),
                })
            },
        }
    }

    fn show_key(&self) -> Result<AndMeta<Self::PublicKey, Self::Metadata>, Self::Error> {
        self.key
            .as_ref()
            .ok_or(Error::NoSuchKey)
            .map(|sealed| AndMeta {
                value: sealed.public_key.clone(),
                metadata: sealed.metadata.clone(),
            })
    }
}
