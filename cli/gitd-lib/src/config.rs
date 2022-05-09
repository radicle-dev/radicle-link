// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{net::SocketAddr, time::Duration};

pub use crate::hooks;

pub struct Config<S> {
    pub paths: librad::paths::Paths,
    pub signer: S,
    pub addr: Option<SocketAddr>,
    pub linger_timeout: Option<Duration>,
    pub network: Network,
}

pub struct Network {
    /// Announce new changes on a `git receive-pack`.
    pub announce: Option<hooks::Announce>,
    /// Make a request-pull call to the configured seeds on a `git
    /// receive-pack`.
    pub request_pull: bool,
    /// Replicate to the configured seeds on a `git upload-pack`.
    pub replicate: bool,
}

impl From<&Network> for hooks::PostReceive {
    fn from(net: &Network) -> Self {
        Self {
            announce: net.announce.clone(),
            request_pull: net.request_pull,
        }
    }
}

impl From<&Network> for hooks::PreUpload {
    fn from(net: &Network) -> Self {
        Self {
            replicate: net.replicate,
        }
    }
}
