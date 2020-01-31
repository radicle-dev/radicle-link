#![allow(clippy::type_complexity)]

use std::convert::Infallible;

use failure::Fail;
use secstr::SecUtf8;

mod crypto;
pub mod error;
pub mod file;
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

pub struct Keypair<PK, SK> {
    pub public_key: PK,
    pub secret_key: SK,
}

pub struct AndMeta<A, M> {
    pub value: A,
    pub metadata: M,
}

pub trait Storage<P>
where
    P: Pinentry,
{
    type PublicKey;
    type SecretKey;

    type Metadata;

    type PutError;
    type GetError;
    type ShowError;

    fn put_keypair(
        &mut self,
        keypair: Keypair<Self::PublicKey, Self::SecretKey>,
        metadata: Self::Metadata,
    ) -> Result<(), Self::PutError>;

    fn get_keypair(
        &self,
    ) -> Result<AndMeta<Keypair<Self::PublicKey, Self::SecretKey>, Self::Metadata>, Self::GetError>;

    fn show_key(&self) -> Result<AndMeta<Self::PublicKey, Self::Metadata>, Self::ShowError>;
}
