use std::{convert::TryFrom, marker::PhantomData};

use crate::{
    crypto::{self, SealedKey},
    AndMeta,
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

pub enum PutError<P> {
    KeyExists,
    Pinentry(P),
}

pub enum GetError<C, P> {
    NoSuchKey,
    Crypto(crypto::Error<C>),
    Pinentry(P),
}

pub enum ShowError {
    NoSuchKey,
}

impl<P, PK, SK, M> Storage<P> for MemoryStorage<P, PK, SK, M>
where
    P: Pinentry,
    SK: AsRef<[u8]> + TryFrom<Vec<u8>>,
    PK: Clone,
    M: Clone,
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
        if self.key.is_some() {
            return Err(Self::PutError::KeyExists);
        }

        let passphrase = self.pinentry.get_passphrase().map_err(PutError::Pinentry)?;
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
    ) -> Result<AndMeta<Keypair<Self::PublicKey, Self::SecretKey>, Self::Metadata>, Self::GetError>
    {
        match &self.key {
            None => Err(GetError::NoSuchKey),
            Some(stored) => {
                let passphrase = self
                    .pinentry
                    .get_passphrase()
                    .map_err(Self::GetError::Pinentry)?;
                let sk = stored
                    .secret_key
                    .unseal(passphrase)
                    .map_err(Self::GetError::Crypto)?;

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

    fn show_key(&self) -> Result<AndMeta<Self::PublicKey, Self::Metadata>, Self::ShowError> {
        self.key
            .as_ref()
            .ok_or(Self::ShowError::NoSuchKey)
            .map(|sealed| AndMeta {
                value: sealed.public_key.clone(),
                metadata: sealed.metadata.clone(),
            })
    }
}
