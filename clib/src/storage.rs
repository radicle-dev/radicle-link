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

/// Intialise a [`ReadOnly`] storage.
pub fn read_only(profile: &Profile) -> Result<ReadOnly, Error> {
    let paths = profile.paths();
    Ok(ReadOnly::open(paths)?)
}

pub mod prompt {
    use super::*;

    /// Initialise [`Storage`].
    ///
    /// The decryption will happen by prompting the person for their passphrase
    /// at the command line.
    pub fn storage(profile: &Profile) -> Result<Storage, Error> {
        let paths = profile.paths();
        let signer = keys::signer_prompt(profile)?;
        Ok(Storage::open(paths, signer)?)
    }
}

pub mod ssh {
    use thrussh_agent::client::ClientStream;

    use super::*;

    /// Initialise [`Storage`].
    ///
    /// The signing key will be retrieved from the ssh-agent. If the key was not
    /// added to the agent then this result in an error.
    pub async fn storage<S>(profile: &Profile) -> Result<Storage, Error>
    where
        S: ClientStream + Unpin + 'static,
    {
        let paths = profile.paths();
        let signer = keys::signer_ssh::<S>(profile).await?;
        Ok(Storage::open(paths, signer)?)
    }
}
