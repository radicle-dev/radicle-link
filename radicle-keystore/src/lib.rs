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

pub trait IntoSecretKey<M> {
    fn into_secret_key(bytes: SecStr, metadata: &M) -> Self;
}

pub trait HasMetadata<M> {
    fn metadata(&self) -> M;
}

pub trait Storage {
    type Pinentry: Pinentry;

    type PublicKey: From<Self::SecretKey>;
    type SecretKey: IntoSecretKey<Self::Metadata> + HasMetadata<Self::Metadata>;

    type Metadata;

    type Error: std::error::Error;

    fn put_key(&mut self, key: Self::SecretKey) -> Result<(), Self::Error>;

    fn get_key(&self) -> Result<Keypair<Self::PublicKey, Self::SecretKey>, Self::Error>;

    fn show_key(&self) -> Result<(Self::PublicKey, Self::Metadata), Self::Error>;
}
