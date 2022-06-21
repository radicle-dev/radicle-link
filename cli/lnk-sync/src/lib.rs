// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{fmt, net::SocketAddr, str::FromStr};

use serde::Serialize;
use thiserror::Error;

use librad::{
    git::Urn,
    net::{
        peer::{client, Client},
        quic::ConnectPeer,
    },
    Signer,
};
use lnk_clib::seed::{Seed, Seeds};

pub mod cli;
mod forked;
pub mod replication;
pub mod request_pull;

/// The successful synchronised state for a given seed.
#[derive(Debug, Serialize)]
pub struct Synced {
    pub seed: Seed<Vec<SocketAddr>>,
    pub replication: Option<replication::Success>,
    pub request_pull: Option<request_pull::Success>,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Replicate(#[from] client::error::Replicate),
    #[error(transparent)]
    RequestPull(#[from] client::error::RequestPull),
}

#[derive(Clone, Copy, Debug)]
pub enum Mode {
    /// Only perform replication from a seed.
    Fetch,
    /// Only perform a request-pull to a seed.
    Push,
    /// Perform both replication and request-pull for a seed.
    All,
}

impl Mode {
    pub fn is_fetch(&self) -> bool {
        match self {
            Mode::Fetch => true,
            Mode::Push => false,
            Mode::All => true,
        }
    }

    pub fn is_push(&self) -> bool {
        match self {
            Mode::Fetch => false,
            Mode::Push => true,
            Mode::All => true,
        }
    }
}

impl Default for Mode {
    fn default() -> Self {
        Self::All
    }
}

impl FromStr for Mode {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "fetch" => Ok(Self::Fetch),
            "push" => Ok(Self::Push),
            "all" => Ok(Self::All),
            _ => Err("invalid mode, exptected one of: ['fetch', 'push', 'all']"),
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Mode::Fetch => "fetch",
            Mode::Push => "push",
            Mode::All => "all",
        })
    }
}

/// Synchronise with the provided list of `seeds` for the given `urn`.
///
/// For each seed the [`Mode`] is checked to see if it should replicate and
/// request-pull.
pub async fn sync<S, E>(client: &Client<S, E>, urn: Urn, seeds: Seeds, mode: Mode) -> Vec<Synced>
where
    S: Signer + Clone,
    E: ConnectPeer + Clone + Send + Sync + 'static,
{
    let mut syncs = Vec::with_capacity(seeds.len());
    let is_push = mode.is_push();
    let is_fetch = mode.is_fetch();
    for seed in seeds.0.into_iter() {
        let replication = if is_fetch {
            match replication::replicate(client, urn.clone(), seed.clone()).await {
                Ok(s) => Some(s),
                Err(err) => {
                    eprintln!(
                        "failed to replicate from the seed: {}, reason: {}",
                        seed.peer, err
                    );
                    tracing::error!(seed = %seed.peer, err = %err, "replication error");
                    None
                },
            }
        } else {
            None
        };

        let request_pull = if is_push {
            match request_pull::request_pull(client, urn.clone(), seed.clone()).await {
                Ok(s) => s,
                Err(err) => {
                    eprintln!(
                        "failed to request-pull to the seed: {}, reason: {}",
                        seed.peer, err
                    );
                    tracing::error!(seed = %seed.peer, err = %err, "request-pull error");
                    None
                },
            }
        } else {
            None
        };

        syncs.push(Synced {
            seed,
            replication,
            request_pull,
        })
    }
    syncs
}
