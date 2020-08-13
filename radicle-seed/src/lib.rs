#[macro_use]
extern crate async_trait;

use std::{net::SocketAddr, vec};
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

#[derive(Debug)]
pub enum Mode {
    /// Track everything we see, no matter where it comes from.
    TrackEverything,
    /// Track everything from these peers, and nothing else.
    TrackPeers(Vec<PeerId>),
    /// Track the specified URNs.
    TrackUrns(Vec<RadUrn>),
}

impl Mode {
    fn is_trackable(&self, peer: &PeerId, urn: &RadUrn) -> bool {
        match self {
            Mode::TrackEverything => true,
            Mode::TrackUrns(ref urns) => urns.contains(urn),
            Mode::TrackPeers(ref peers) => peers.contains(peer),
        }
    }
}

#[derive(Debug)]
pub struct NodeConfig {
    /// Address to listen to for new connections.
    pub listen_addr: SocketAddr,
    /// Operational mode. Determines the tracking rules for this seed node.
    pub mode: Mode,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            listen_addr: ([0, 0, 0, 0], 0).into(),
            mode: Mode::TrackEverything,
        }
    }
}

pub struct Node {
    config: NodeConfig,
}

impl Node {
    pub fn new(config: NodeConfig) -> Result<Self, Error> {
        Ok(Node { config })
    }

    pub async fn run(self) -> Result<(), Error> {
        use futures::stream::StreamExt;

        let paths = paths::Paths::new()?;
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

        // Start by tracking specified projects if we need to.
        if let Mode::TrackUrns(urns) = &mode {
            for urn in urns {
                let mut peers = api.providers(urn.clone()).await;
                while let Some(peer) = peers.next().await {
                    Node::track_project(&api, urn, &peer)?;
                }
            }
        }

        // Listen on gossip events. As soon as a peer announces something of interest,
        // we check if we should track it.
        while let Some(event) = events.next().await {
            match event {
                ProtocolEvent::Gossip(gossip::Info::Has(gossip::Has { provider, val })) => {
                    let urn = &val.urn;
                    let peer_id = &provider.peer_id;

                    if mode.is_trackable(peer_id, urn) {
                        Node::track_project(&api, urn, &provider)?;
                    }
                },
                _ => {},
            }
        }
        Ok(())
    }

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

        api.storage()
            .clone_repo::<project::ProjectInfo, _>(url, addr_hints)?;
        api.storage().track(urn, &peer_id)?;

        Ok(())
    }
}
