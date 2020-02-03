use std::convert::Infallible;

use secstr::{SecStr, SecUtf8};

mod crypto;
pub mod file;
pub mod memory;

pub use file::FileStorage;
pub use memory::MemoryStorage;

/// A method to obtain a passphrase from which an encryption key can be derived.
///
/// Similar in spirit to GPG's `pinentry` program, but no implementation of the
/// Assuan protocol is provided as of yet.
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

/// Named pair of public / secret key.
pub struct Keypair<PK, SK> {
    pub public_key: PK,
    pub secret_key: SK,
}

pub trait IntoSecretKey
where
    Self: Sized,
{
    type Metadata;
    type Error;

    fn into_secret_key(bytes: SecStr, metadata: &Self::Metadata) -> Result<Self, Self::Error>;
}

pub trait HasMetadata {
    type Metadata;

    fn metadata(&self) -> Self::Metadata;
}

pub trait Storage {
    type Pinentry: Pinentry;

    type PublicKey: From<Self::SecretKey>;
    type SecretKey: IntoSecretKey<Metadata = Self::Metadata>
        + HasMetadata<Metadata = Self::Metadata>;

    type Metadata;

    type Error: std::error::Error;

    fn put_key(&mut self, key: Self::SecretKey) -> Result<(), Self::Error>;

    fn get_key(&self) -> Result<Keypair<Self::PublicKey, Self::SecretKey>, Self::Error>;

    fn show_key(&self) -> Result<(Self::PublicKey, Self::Metadata), Self::Error>;
}
