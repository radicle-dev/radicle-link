use std::{convert::Infallible, time::SystemTime};

use failure::Fail;
use secstr::SecUtf8;

mod crypto;
pub mod error;
pub mod memory;

pub trait Pinentry {
    type Error: Fail;

    fn get_passphrase(&self) -> Result<SecUtf8, Self::Error>;
}

impl Pinentry for SecUtf8 {
    type Error = Infallible;

    fn get_passphrase(&self) -> Result<SecUtf8, Infallible> {
        Ok(self.clone())
    }
}

pub type Keypair<PK, SK> = (PK, SK);

pub trait Storage<P>
where
    P: Pinentry,
{
    type PublicKey;
    type SecretKey;

    type PutError;
    type GetError;
    type ShowError;

    fn put_keypair(
        &mut self,
        keypair: Keypair<Self::PublicKey, Self::SecretKey>,
        created_at: Option<SystemTime>,
    ) -> Result<(), Self::PutError>;

    fn get_keypair(&self) -> Result<Keypair<Self::PublicKey, Self::SecretKey>, Self::GetError>;
    fn show_key(&self) -> Result<Self::PublicKey, Self::ShowError>;
}
