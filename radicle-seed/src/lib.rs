#[macro_use]
extern crate async_trait;

use std::{net::SocketAddr, vec};
use thiserror::Error;

use radicle_keystore::sign::ed25519;

use librad::{
    git,
    keys,
    net::{
        discovery,
        gossip,
        peer::{self, PeerConfig},
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
pub struct Signer {}

impl Signer {
    fn new() -> Self {
        Self {}
    }
}

#[derive(Error, Debug)]
pub enum SignerError {
    #[error("error")]
    Error(()),
}

#[async_trait]
impl ed25519::Signer for Signer {
    type Error = SignerError;

    fn public_key(&self) -> ed25519::PublicKey {
        todo!()
    }

    async fn sign(&self, _data: &[u8]) -> Result<ed25519::Signature, Self::Error> {
        todo!()
    }
}

impl keys::AsPKCS8 for Signer {
    fn as_pkcs8(&self) -> Vec<u8> {
        todo!()
    }
}

/// Short-hand type for the peer discovery subsystem.
type Disco = discovery::Static<vec::IntoIter<(PeerId, SocketAddr)>, SocketAddr>;

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

pub struct NodeConfig {
    /// Address to listen to for new connections.
    listen_addr: SocketAddr,
    /// Operational mode. Determines the tracking rules for this seed node.
    mode: Mode,
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
    config: PeerConfig<Disco, Signer>,
    mode: Mode,
}

impl Node {
    pub fn new(cfg: NodeConfig) -> Result<Self, Error> {
        let paths = paths::Paths::new()?;
        let gossip_params = Default::default();
        let seeds: Vec<(PeerId, SocketAddr)> = vec![];
        let disco = discovery::Static::new(seeds);

        let signer = Signer::new();
        let config = PeerConfig {
            signer,
            paths,
            listen_addr: cfg.listen_addr,
            gossip_params,
            disco,
        };

        Ok(Node {
            config,
            mode: cfg.mode,
        })
    }

    pub async fn run(self) -> Result<(), Error> {
        use futures::stream::StreamExt;

        let peer = self.config.try_into_peer().await?;

        let (api, future) = peer.accept()?;
        let mut events = api.protocol().subscribe().await;
        let mode = self.mode;

        // TODO: Query for URNs.

        // Spawn the background peer thread.
        tokio::spawn(future);

        while let Some(event) = events.next().await {
            match event {
                ProtocolEvent::Gossip(gossip::Info::Has(gossip::Has { provider, val })) => {
                    if mode.is_trackable(&provider.peer_id, &val.urn) {
                        // TODO: Track URN.
                    }
                },
                _ => {},
            }
        }
        Ok(())
    }
}
