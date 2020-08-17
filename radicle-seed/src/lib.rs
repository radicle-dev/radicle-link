#[macro_use]
extern crate async_trait;

use futures::stream::StreamExt;
use std::{collections::HashSet, net::SocketAddr, path::PathBuf, vec};
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
    fn new() -> Self {
        let key = keys::SecretKey::new();
        Self { key }
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
#[derive(Debug)]
pub struct NodeConfig {
    /// Address to listen to for new connections.
    pub listen_addr: SocketAddr,
    /// Operational mode. Determines the tracking rules for this seed node.
    pub mode: Mode,
    /// Radicle root path.
    pub root: Option<PathBuf>,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            listen_addr: ([0, 0, 0, 0], 0).into(),
            mode: Mode::TrackEverything,
            root: None,
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
        let signer = Signer::new();
        let config = PeerConfig {
            signer,
            paths,
            listen_addr: self.config.listen_addr,
            gossip_params,
            disco,
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
                        Node::track_project(&api, urn, &provider).ok();
                    }
                },
                _ => {},
            }
        }
        Ok(())
    }

    /// Attempt to track a project.
    fn track_project(
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
            .map(|a: &std::net::IpAddr| (*a, port).into());

        let result = api
            .storage()
            .clone_repo::<project::ProjectInfo, _>(url, addr_hints)
            .and_then(|_| api.storage().track(urn, &peer_id));

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
                    let mut peers = api.providers(urn.clone()).await;
                    // Attempt to track until we succeed.
                    while let Some(peer) = peers.next().await {
                        if Node::track_project(&api, urn, &peer).is_ok() {
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
