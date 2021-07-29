// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use thiserror::Error;

use librad::{
    git::storage::{error, read, ReadOnly, Storage},
    profile::Profile,
};

use super::keys;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    ReadInit(#[from] read::error::Init),
    #[error(transparent)]
    ReadWriteInit(#[from] error::Init),
    #[error(transparent)]
    Keys(#[from] super::keys::Error),
}

/// How to decrypt the secret key from the file store when initialising the
/// [`Storage`].
pub enum Crypto {
    /// The decryption will happen by prompting the person for their passphrase
    /// at the command line.
    Prompt,
    // TODO(finto): SshAgent
}

/// Intialise a [`ReadOnly`] storage.
pub fn read_only(profile: &Profile) -> Result<ReadOnly, Error> {
    let paths = profile.paths();
    Ok(ReadOnly::open(paths)?)
}

/// Initialise [`Storage`] based on the [`Crypto`] provided.
pub fn read_write(profile: &Profile, crypto: Crypto) -> Result<Storage, Error> {
    let paths = profile.paths();
    match crypto {
        Crypto::Prompt => {
            let signer = keys::signer_prompt(profile)?;
            Ok(Storage::open(paths, signer)?)
        },
    }
}
