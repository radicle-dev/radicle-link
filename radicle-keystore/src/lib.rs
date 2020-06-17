// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

//! `radicle-keystore` aims to become the sole abstraction over storage of key
//! material in the Radicle ecosystem.
//!
//! Radicle employs two kinds of keys: ones which may leave your device (e.g.
//! onto an HSM), and ones that shouldn't. For the first kind, we will
//! eventually provide an implementation of [`Keystore`] which interfaces
//! directly with system keychains or hardware devices, while for the second
//! kind matters are a bit more complicated: we recommend to use the
//! [`file::FileStorage`] implementation, which stores keys in encrypted form on
//! the filesystem. This is to discourage (accidental) key sharing via backup or
//! cross-device syncing setups the user might have.
//!
//! The choice of [`crypto::Crypto`] (and relatedly [`pinentry::Pinentry`]) may
//! however be used to store the passphrase for a key-derivation scheme (as
//! employed by [`crypto::Pwhash`]) in some system keychain, or offload
//! encryption entirely to an external system (such as GPG, or a password
//! manager).
pub use secstr::SecStr;

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

    /// Securely store secret key `key` in the keystore.
    ///
    /// The key may carry [`Keystore::Metadata`], which is stored alongside the
    /// key material. The metadata, as well as the public portion of the
    /// key, may be stored in clear, so as to not require prompting the user
    /// when retrieving those values.
    ///
    /// Key rotation is not (yet) part of this API, thus `put_key` MUST return
    /// an error if an equivalent key is already present in the storage
    /// backend.
    fn put_key(&mut self, key: Self::SecretKey) -> Result<(), Self::Error>;

    /// Retrieve both the secret and public parts of the stored key material.
    fn get_key(&self) -> Result<Keypair<Self::PublicKey, Self::SecretKey>, Self::Error>;

    /// Retrieve only the public part of the key material, along with any
    /// metadata.
    fn show_key(&self) -> Result<(Self::PublicKey, Self::Metadata), Self::Error>;
}
