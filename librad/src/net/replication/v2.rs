// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{net::SocketAddr, time::Duration};

use link_async::Spawner;

use crate::{
    git::{
        self,
        identities::local::LocalIdentity,
        replication as legacy,
        storage::{
            fetcher::{self, retrying, Fetchers},
            Pooled,
            Storage,
        },
    },
    identities::git::Urn,
    PeerId,
};

pub use legacy::{IdStatus, Mode};

pub mod error {
    use super::*;
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum Replicate {
        #[error(transparent)]
        Retrying(#[from] fetcher::error::Retrying<git2::Error>),

        #[error(transparent)]
        Replication(#[from] legacy::Error),
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub limit: git::fetch::Limit,
    pub wait_slot: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            limit: git::fetch::Limit::default(),
            wait_slot: Duration::from_secs(20),
        }
    }
}

pub type Success = legacy::ReplicateResult;

#[derive(Clone)]
pub struct Replication {
    config: Config,
    fetchers: Fetchers,
}

impl Replication {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            fetchers: Fetchers::default(),
        }
    }

    pub async fn replicate<P>(
        &self,
        spawner: &Spawner,
        pool: &P,
        from: impl Into<(PeerId, Vec<SocketAddr>)>,
        urn: Urn,
        whoami: Option<LocalIdentity>,
    ) -> Result<Success, error::Replicate>
    where
        P: Pooled<Storage> + Send + 'static,
    {
        let (remote_peer, addr_hints) = from.into();
        let res = retrying(
            spawner,
            self.fetchers.clone(),
            pool,
            fetcher::PeerToPeer::new(urn, remote_peer, addr_hints),
            self.config.wait_slot,
            {
                let config = legacy::Config {
                    fetch_limit: self.config.limit,
                };
                move |storage, fetcher| legacy::replicate(storage, fetcher, config, whoami.clone())
            },
        )
        .await;

        Ok(res??)
    }
}
