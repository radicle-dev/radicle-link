// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom, io, net::SocketAddr};

use librad::net::discovery;
use tokio::net::lookup_host;

use librad::PeerId;

use crate::args::Bootstrap;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the seed `{0}` failed to resolve to an address")]
    DnsLookupFailed(String),

    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Seed {
    pub addrs: Vec<SocketAddr>,
    pub peer_id: PeerId,
}

impl Seed {
    /// Create a [`Seed`] from a [`Bootstrap`].
    ///
    /// # Errors
    ///
    /// * If the supplied address cannot be resolved.
    async fn try_from_bootstrap(bootstrap: &Bootstrap) -> Result<Self, Error> {
        let addrs: Vec<SocketAddr> = lookup_host(bootstrap.addr.clone()).await?.collect();
        if !addrs.is_empty() {
            Ok(Self {
                addrs,
                peer_id: bootstrap.peer_id,
            })
        } else {
            Err(Error::DnsLookupFailed(bootstrap.to_string()))
        }
    }
}

pub struct Seeds(pub Vec<Seed>);

impl Seeds {
    pub async fn resolve(bootstraps: &[Bootstrap]) -> Result<Self, Error> {
        let mut resolved = Vec::with_capacity(bootstraps.len());

        for bootstrap in bootstraps.iter() {
            resolved.push(Seed::try_from_bootstrap(bootstrap).await?);
        }

        Ok(Self(resolved))
    }
}

impl TryFrom<Seeds> for discovery::Static {
    type Error = Error;

    fn try_from(seeds: Seeds) -> Result<Self, Self::Error> {
        discovery::Static::resolve(
            seeds
                .0
                .iter()
                .map(|seed| (seed.peer_id, seed.addrs.as_slice())),
        )
        .map_err(Error::from)
    }
}
