// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

#![allow(clippy::single_match)]
#[macro_use]
extern crate async_trait;

use std::{collections::HashSet, io, net::SocketAddr, path::PathBuf, time::Duration, vec};

use futures::stream::StreamExt;
use thiserror::Error;

use radicle_keystore::sign::ed25519;

use librad::{
    git,
    keys,
    meta::project,
    net::{
        discovery,
        gossip,
        gossip::types::PeerInfo,
        peer::{self, PeerApi, PeerConfig},
        protocol::ProtocolEvent,
    },
    paths,
    peer::PeerId,
    uri::RadUrn,
};

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Api(#[from] peer::ApiError),

    #[error(transparent)]
    Storage(#[from] git::storage::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Bootstrap(#[from] peer::BootstrapError),

    #[error(transparent)]
    Accept(#[from] peer::AcceptError),
}

#[derive(Clone)]
pub struct Signer {
    key: keys::SecretKey,
}

impl Signer {
    pub fn new<R: io::Read>(mut r: R) -> Result<Self, io::Error> {
        use radicle_keystore::SecretKeyExt;

        let mut bytes = Vec::new();
        r.read_to_end(&mut bytes)?;

        let sbytes: keys::SecStr = bytes.into();
        match keys::SecretKey::from_bytes_and_meta(sbytes, &()) {
            Ok(key) => Ok(Self { key }),
            Err(err) => Err(io::Error::new(io::ErrorKind::InvalidData, err)),
        }
    }
}

#[async_trait]
impl ed25519::Signer for Signer {
    type Error = std::convert::Infallible;

    fn public_key(&self) -> ed25519::PublicKey {
        self.key.public_key()
    }

    async fn sign(&self, data: &[u8]) -> Result<ed25519::Signature, Self::Error> {
        <keys::SecretKey as ed25519::Signer>::sign(&self.key, data).await
    }
}

impl keys::AsPKCS8 for Signer {
    fn as_pkcs8(&self) -> Vec<u8> {
        self.key.as_pkcs8()
    }
}

/// Seed operational mode.
#[derive(Debug)]
pub enum Mode {
    /// Track everything we see, no matter where it comes from.
    TrackEverything,
    /// Track everything from these peers, and nothing else.
    TrackPeers(HashSet<PeerId>),
    /// Track the specified URNs.
    TrackUrns(HashSet<RadUrn>),
}

impl Mode {
    /// Returns whether or not a given peer/URN pair should be tracked or not.
    fn is_trackable(&self, peer: &PeerId, urn: &RadUrn) -> bool {
        match self {
            Mode::TrackEverything => true,
            Mode::TrackUrns(ref urns) => urns.contains(urn),
            Mode::TrackPeers(ref peers) => peers.contains(peer),
        }
    }
}

/// Node configuration.
pub struct NodeConfig {
    /// Address to listen to for new connections.
    pub listen_addr: SocketAddr,
    /// Operational mode. Determines the tracking rules for this seed node.
    pub mode: Mode,
    /// Radicle root path.
    pub root: Option<PathBuf>,
    /// Signer.
    pub signer: Signer,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            listen_addr: ([0, 0, 0, 0], 0).into(),
            mode: Mode::TrackEverything,
            root: None,
            signer: Signer {
                key: keys::SecretKey::new(),
            },
        }
    }
}

/// Seed node instance.
pub struct Node {
    config: NodeConfig,
}

impl Node {
    /// Create a new seed node.
    pub fn new(config: NodeConfig) -> Result<Self, Error> {
        Ok(Node { config })
    }

    /// Run the seed node. This function runs indefinitely until a fatal error
    /// occurs.
    pub async fn run(self) -> Result<(), Error> {
        let paths = if let Some(root) = self.config.root {
            paths::Paths::from_root(root)?
        } else {
            paths::Paths::new()?
        };
        let gossip_params = Default::default();
        let seeds: Vec<(PeerId, SocketAddr)> = vec![];
        let disco = discovery::Static::new(seeds);
        let storage_config = Default::default();
        let config = PeerConfig {
            signer: self.config.signer,
            paths,
            listen_addr: self.config.listen_addr,
            gossip_params,
            disco,
            storage_config,
        };

        let peer = config.try_into_peer().await?;

        let (api, future) = peer.accept()?;
        let mut events = api.protocol().subscribe().await;
        let mode = self.config.mode;

        // Spawn the background peer thread.
        tokio::spawn(future);

        // Track already-known URNs.
        Node::initialize_tracker(&mode, &api).await?;

        // Listen on gossip events. As soon as a peer announces something of interest,
        // we check if we should track it.
        while let Some(event) = events.next().await {
            match event {
                ProtocolEvent::Gossip(gossip::Info::Has(gossip::Has { provider, val })) => {
                    let urn = &val.urn;
                    let peer_id = &provider.peer_id;

                    tracing::info!("Discovered new URN {} from peer {}", urn, peer_id);

                    if mode.is_trackable(peer_id, urn) {
                        // Attempt to track, but keep going if it fails.
                        Node::track_project(&api, urn, &provider).await.ok();
                    }
                },
                _ => {},
            }
        }
        Ok(())
    }

    /// Attempt to track a project.
    async fn track_project(
        api: &PeerApi<Signer>,
        urn: &RadUrn,
        peer_info: &PeerInfo<std::net::IpAddr>,
    ) -> Result<(), Error> {
        let peer_id = &peer_info.peer_id;
        let url = urn.clone().into_rad_url(peer_id.clone());
        let port = peer_info.advertised_info.listen_port;
        let addr_hints = peer_info
            .seen_addrs
            .iter()
            .map(|a: &std::net::IpAddr| (*a, port).into())
            .collect::<Vec<_>>();

        let result = {
            let peer_id = peer_id.clone();
            let urn = urn.clone();
            api.with_storage(move |storage| {
                storage
                    .clone_repo::<project::ProjectInfo, _>(url, addr_hints)
                    .and_then(|_| storage.track(&urn, &peer_id))
            })
        }
        .await
        .expect("`clone_repo` panicked");

        match &result {
            Ok(()) => {
                tracing::info!("Successfully tracked project {} from peer {}", urn, peer_id,);
            },
            Err(err) => {
                tracing::debug!(
                    "Error tracking project {} from peer {}: {}",
                    urn,
                    peer_id,
                    err
                );
            },
        }
        result.map_err(Error::from)
    }

    /// Attempt to track initial URN list, if any.
    async fn initialize_tracker(mode: &Mode, api: &PeerApi<Signer>) -> Result<(), Error> {
        // Start by tracking specified projects if we need to.
        match &mode {
            Mode::TrackUrns(urns) => {
                tracing::info!("Initializing tracker with {} URNs..", urns.len());

                for urn in urns {
                    let mut peers = api.providers(urn.clone(), Duration::from_secs(30)).await;
                    // Attempt to track until we succeed.
                    while let Some(peer) = peers.next().await {
                        if Node::track_project(&api, urn, &peer).await.is_ok() {
                            break;
                        }
                    }
                }
            },
            Mode::TrackPeers(peers) => {
                // Nb. We don't proactively track peers in this mode, we wait for them
                // to announce URNs instead.
                tracing::info!("Initializing tracker with {} peers..", peers.len());
            },
            Mode::TrackEverything => {
                tracing::info!("Initializing tracker to track everything..");
            },
        }
        Ok(())
    }
}
