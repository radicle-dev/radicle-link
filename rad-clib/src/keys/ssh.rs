// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    convert::Infallible,
    fmt,
    path::{Path, PathBuf},
    str::FromStr,
};

use thiserror::Error;

use librad::{
    crypto::{keystore::sign::ssh, BoxedSignError},
    git::storage::read,
    PeerId,
};

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
    SignError(#[from] BoxedSignError),
    #[error(transparent)]
    SshConnect(#[from] ssh::error::Connect),
    #[error(transparent)]
    StorageInit(#[from] read::error::Init),
}

/// Which unix domain socket the `ssh-agent` should connect to.
///
/// When this value is `Env` it will use the `SSH_AUTH_SOCK` environment
/// variable. When this value is `Uds` it will use the path provided.
///
/// # Default
///
/// The default value for this `Env`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SshAuthSock {
    Env,
    Uds(PathBuf),
}

impl fmt::Display for SshAuthSock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Env => write!(f, "env"),
            Self::Uds(path) => write!(f, "{}", path.display()),
        }
    }
}

impl FromStr for SshAuthSock {
    type Err = Infallible;

    fn from_str(val: &str) -> Result<Self, Self::Err> {
        match val {
            "env" => Ok(Self::Env),
            s => Ok(Self::Uds(Path::new(s).to_path_buf())),
        }
    }
}

impl Default for SshAuthSock {
    fn default() -> Self {
        Self::Env
    }
}

fn with_socket(agent: ssh::SshAgent, sock: SshAuthSock) -> ssh::SshAgent {
    match sock {
        SshAuthSock::Env => agent,
        SshAuthSock::Uds(path) => agent.with_path(path),
    }
}

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;

#[cfg(not(unix))]
mod win;
#[cfg(not(unix))]
pub use win::*;
