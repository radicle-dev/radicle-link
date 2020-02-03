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

/// Abstraction over secure storage for private key material.
pub trait Keystore {
    type Pinentry: Pinentry;

    type PublicKey: From<Self::SecretKey>;
    type SecretKey: IntoSecretKey<Metadata = Self::Metadata>
        + HasMetadata<Metadata = Self::Metadata>;

    type Metadata;

    type Error: std::error::Error;

    /// Store secret key `key` in the keystore.
    ///
    /// The key may carry [`Self::Metadata`], which is stored alongside the key
    /// material. The secret key material MUST be encrypted using a key
    /// derived from the passphrase obtained via [`Self::Pinentry`]
    ///
    /// The public portion of the key (as obtained via
    /// [`From<Self::SecretKey>`]) and the metadata may be stored in plain text.
    ///
    /// `put_key` MUST return an error if an equivalent key is already present
    /// in the storage backend (i.e. key deletion / rotation is not provided
    /// by this interface).
    fn put_key(&mut self, key: Self::SecretKey) -> Result<(), Self::Error>;

    /// Retrieve both the secret and public parts of the stored key material.
    ///
    /// In order to decrypt the secret key, [`Self::Pinentry`] shall be invoked.
    fn get_key(&self) -> Result<Keypair<Self::PublicKey, Self::SecretKey>, Self::Error>;

    /// Retrieve only the public part of the key material, along with any
    /// metadata.
    fn show_key(&self) -> Result<(Self::PublicKey, Self::Metadata), Self::Error>;
}
