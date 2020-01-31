use std::time::SystemTime;

use crate::{crypto::SealedKey, Keypair, Pinentry, Storage};

pub struct MemoryStorage<Pin, PK, SK> {
    key: Option<SealedKey<PK, SK>>,
    pinentry: Pin,
}

impl<Pin, PK, SK> MemoryStorage<Pin, PK, SK> {
    pub fn new(pinentry: Pin) -> Self {
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

pub enum GetError<P> {
    NoSuchKey,
    Crypto(failure::Error),
    Pinentry(P),
}

pub enum ShowError {
    NoSuchKey,
}

impl<P, PK, SK> Storage<P> for MemoryStorage<P, PK, SK>
where
    P: Pinentry,
    SK: AsRef<[u8]> + From<Vec<u8>>,
    PK: Clone,
{
    type PublicKey = PK;
    type SecretKey = SK;

    type PutError = PutError<P::Error>;
    type GetError = GetError<P::Error>;
    type ShowError = ShowError;

    fn put_keypair(
        &mut self,
        keypair: Keypair<Self::PublicKey, Self::SecretKey>,
        created_at: Option<SystemTime>,
    ) -> Result<(), Self::PutError> {
        if self.key.is_some() {
            return Err(Self::PutError::KeyExists);
        }

        let passphrase = self.pinentry.get_passphrase().map_err(PutError::Pinentry)?;
        let created_at = created_at.unwrap_or_else(SystemTime::now);
        self.key = Some(SealedKey::seal(
            keypair.0, keypair.1, passphrase, created_at,
        ));

        Ok(())
    }

    fn get_keypair(&self) -> Result<Keypair<Self::PublicKey, Self::SecretKey>, Self::GetError> {
        match &self.key {
            None => Err(GetError::NoSuchKey),
            Some(sealed) => {
                let passphrase = self
                    .pinentry
                    .get_passphrase()
                    .map_err(Self::GetError::Pinentry)?;
                let sk = sealed.unseal(passphrase).map_err(Self::GetError::Crypto)?;
                Ok((sealed.public_key.clone(), sk))
            },
        }
    }

    fn show_key(&self) -> Result<Self::PublicKey, Self::ShowError> {
        self.key
            .as_ref()
            .ok_or(Self::ShowError::NoSuchKey)
            .map(|sealed| sealed.public_key.clone())
    }
}
