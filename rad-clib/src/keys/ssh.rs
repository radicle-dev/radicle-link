// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use thiserror::Error;

use librad::{crypto::keystore::sign::ssh, git::storage::read, PeerId};

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    AddKey(#[from] ssh::error::AddKey),
    #[error("failed to get the key material from your file storage")]
    GetKey(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),
    #[error(transparent)]
    ListKeys(#[from] ssh::error::ListKeys),
    #[error(
        "the key for {0} is not in the ssh-agent, consider adding it via `rad profile ssh-add`"
    )]
    NoSuchKey(PeerId),
    #[error(transparent)]
    RemoveKey(#[from] ssh::error::RemoveKey),
    #[error(transparent)]
    SshConnect(#[from] ssh::error::Connect),
    #[error(transparent)]
    StorageInit(#[from] read::error::Init),
}

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;

#[cfg(not(unix))]
mod win;
#[cfg(not(unix))]
pub use win::*;
