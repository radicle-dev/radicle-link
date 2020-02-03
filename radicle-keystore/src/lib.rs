#![allow(clippy::type_complexity)]

use std::convert::Infallible;

use secstr::{SecStr, SecUtf8};

mod crypto;
pub mod file;
pub mod memory;

pub use file::FileStorage;
pub use memory::MemoryStorage;

pub trait Pinentry {
    type Error: std::error::Error;

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

#[derive(Debug)]
pub struct AndMeta<A, M> {
    pub value: A,
    pub metadata: M,
}

pub trait IntoSecretKey<M> {
    fn into_secret_key(bytes: SecStr, metadata: &M) -> Self;
}

pub trait Storage {
    type Pinentry: Pinentry;

    type PublicKey;
    type SecretKey: IntoSecretKey<Self::Metadata>;

    type Metadata;

    type Error: std::error::Error;

    fn put_keypair(
        &mut self,
        keypair: Keypair<Self::PublicKey, Self::SecretKey>,
        metadata: Self::Metadata,
    ) -> Result<(), Self::Error>;

    fn get_keypair(
        &self,
    ) -> Result<AndMeta<Keypair<Self::PublicKey, Self::SecretKey>, Self::Metadata>, Self::Error>;

    fn show_key(&self) -> Result<AndMeta<Self::PublicKey, Self::Metadata>, Self::Error>;
}
