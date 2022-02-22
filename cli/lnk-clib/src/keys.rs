// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::{
    crypto::keystore::{crypto::Crypto, FileStorage},
    profile::Profile,
    PublicKey,
    SecretKey,
};

pub mod prompt;
pub mod ssh;

/// The filename for storing the secret key.
pub const LIBRAD_KEY_FILE: &str = "librad.key";

/// Create a [`FileStorage`] for [`SecretKey`]s.
pub fn file_storage<C>(profile: &Profile, crypto: C) -> FileStorage<C, PublicKey, SecretKey, ()>
where
    C: Crypto,
{
    FileStorage::new(&profile.paths().keys_dir().join(LIBRAD_KEY_FILE), crypto)
}
