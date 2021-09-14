// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use thiserror::Error;

use librad::{crypto::keystore::sign::ssh, git::storage::read};

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    SshConnect(#[from] ssh::error::Connect),
    #[error(transparent)]
    AddKey(#[from] ssh::error::AddKey),
    #[error(transparent)]
    GetKey(#[from] Box<dyn std::error::Error + Send + Sync + 'static>),
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
