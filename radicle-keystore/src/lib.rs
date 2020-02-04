use secstr::SecStr;

pub mod crypto;
pub mod file;
pub mod memory;
pub mod pinentry;

#[cfg(test)]
pub(crate) mod test;

pub use file::FileStorage;
pub use memory::MemoryStorage;

/// Named pair of public / secret key.
pub struct Keypair<PK, SK> {
    pub public_key: PK,
    pub secret_key: SK,
}

pub trait SecretKeyExt: Sized {
    type Metadata;
    type Error;

    fn from_bytes_and_meta(bytes: SecStr, metadata: &Self::Metadata) -> Result<Self, Self::Error>;
    fn metadata(&self) -> Self::Metadata;
}

/// Abstraction over secure storage for private key material.
pub trait Keystore {
    type PublicKey: From<Self::SecretKey>;
    type SecretKey: SecretKeyExt<Metadata = Self::Metadata>;

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
