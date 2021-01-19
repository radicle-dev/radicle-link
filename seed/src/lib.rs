// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#![allow(clippy::single_match)]
#[macro_use]
extern crate async_trait;

pub mod event;
pub mod handle;
pub mod project;
pub mod signer;

use std::{collections::HashSet, net::SocketAddr, path::PathBuf, time::Duration, vec};

use futures::{channel::mpsc as chan, select, sink::SinkExt as _, stream::StreamExt as _};
use thiserror::Error;

use librad::{
    git::{
        identities::{self, Person, SomeIdentity, Urn},
        replication,
        storage,
        tracking,
    },
    net::{
        discovery,
        gossip,
        gossip::types::PeerInfo,
        peer::{self, Peer, PeerApi, PeerConfig},
        protocol::ProtocolEvent,
        Network,
    },
    paths,
    peer::PeerId,
    profile,
};

pub use crate::{
    event::Event,
    handle::{NodeError, NodeHandle, Request},
    project::Project,
    signer::Signer,
};

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
    #[error("unable to resolve URN {0}")]
    NoSuchUrn(Urn),

    #[error(transparent)]
    Api(#[from] peer::ApiError),

    #[error(transparent)]
    Storage(#[from] storage::Error),

    #[error(transparent)]
    Tracking(#[from] tracking::Error),

    #[error(transparent)]
    Identities(#[from] identities::Error),

    #[error(transparent)]
    Replication(#[from] replication::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Bootstrap(#[from] peer::BootstrapError),

    #[error(transparent)]
    Accept(#[from] peer::AcceptError),

    #[error(transparent)]
    Node(#[from] NodeError),

    #[error(transparent)]
    Channel(#[from] chan::SendError),

    #[error(transparent)]
    Profile(#[from] profile::Error),
}

/// Seed operational mode.
#[derive(Debug)]
pub enum Mode {
    /// Track everything we see, no matter where it comes from.
    TrackEverything,
    /// Track everything from these peers, and nothing else.
    TrackPeers(HashSet<PeerId>),
    /// Track the specified URNs.
    TrackUrns(HashSet<Urn>),
}

impl Mode {
    /// Returns whether or not a given peer/URN pair should be tracked or not.
    fn is_trackable(&self, peer: &PeerId, urn: &Urn) -> bool {
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
    /// The radicle network to connect to.
    pub network: Network,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            listen_addr: ([0, 0, 0, 0], 0).into(),
            mode: Mode::TrackEverything,
            root: None,
            network: Network::default(),
        }
    }
}

/// Seed node instance.
pub struct Node {
    /// Node configuration.
    config: NodeConfig,
    /// Peer configuration.
    peer_config: PeerConfig<Signer>,
    /// Receiver end of user requests.
    requests: chan::UnboundedReceiver<Request>,
    /// Sender end of user requests.
    handle: chan::UnboundedSender<Request>,
}

impl Node {
    /// Create a new seed node.
    pub fn new(config: NodeConfig, signer: Signer) -> Result<Self, Error> {
        let (handle, requests) = chan::unbounded::<Request>();
        let paths = if let Some(root) = &config.root {
            paths::Paths::from_root(root)?
        } else {
            profile::Profile::load()?.paths().to_owned()
        };
        let gossip_params = Default::default();
        let storage_config = Default::default();
        let fetch_limit = Default::default();
        let peer_config = PeerConfig {
            signer,
            paths,
            listen_addr: config.listen_addr,
            gossip_params,
            storage_config,
            fetch_limit,
            network: config.network,
        };

        Ok(Node {
            peer_config,
            config,
            handle,
            requests,
        })
    }

    /// Get the node's peer id.
    pub fn peer_id(&self) -> PeerId {
        PeerId::from_signer(&self.peer_config.signer)
    }

    /// Create a new handle.
    pub fn handle(&self) -> NodeHandle {
        NodeHandle::new(self.handle.clone())
    }

    /// Run the seed node. This function runs indefinitely until a fatal error
    /// occurs.
    pub async fn run(self, mut transmit: chan::Sender<Event>) -> Result<(), Error> {
        let peer = Peer::bootstrap(self.peer_config, discovery::Static::from(vec![])).await?;
        let (api, future) = peer.accept()?;
        let mut events = api.protocol().subscribe().await.fuse();
        let mut requests = self.requests;
        let mode = &self.config.mode;

        // Spawn the background peer thread.
        tokio::spawn(future);

        // Track already-known URNs.
        Node::initialize_tracker(mode, &api, &mut transmit).await?;

        loop {
            select! {
                event = events.next() => {
                    if let Some(e) = event {
                        Node::handle_event(e, mode, &mut transmit, &api).await?;
                    } else {
                        // If the Peer API isn't closed its end of the channel, we're done.
                        break;
                    }
                }
                request = requests.next() => {
                    if let Some(r) = request {
                        Node::handle_request(r, &api).await?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Handle user requests.
    async fn handle_request(request: Request, api: &PeerApi) -> Result<(), Error> {
        match request {
            Request::GetPeers(mut reply) => {
                let peers = api.protocol().connected_peers().await;
                reply.send(peers).await?;
            },
            Request::GetProjects(mut reply) => {
                let projs = project::get_projects(api).await?;
                reply.send(projs).await?;
            },
        }
        Ok(())
    }

    /// Handle gossip events. As soon as a peer announces something of interest,
    /// we check if we should track it.
    async fn handle_event(
        event: ProtocolEvent<peer::types::Gossip>,
        mode: &Mode,
        transmit: &mut chan::Sender<Event>,
        api: &PeerApi,
    ) -> Result<(), Error> {
        match event {
            ProtocolEvent::Gossip(gossip::Info::Has(gossip::Has { provider, val })) => {
                let urn = &val.urn;
                let peer_id = &provider.peer_id;

                tracing::info!("Discovered new URN {} from peer {}", urn, peer_id);

                if mode.is_trackable(peer_id, urn) {
                    // Attempt to track, but keep going if it fails.
                    if Node::track_project(&api, urn, &provider).await.is_ok() {
                        let event = Event::project_tracked(urn.clone(), *peer_id, &api).await?;
                        transmit.send(event).await.ok();
                    }
                }
            },
            ProtocolEvent::Connected(id) => {
                let event = Event::peer_connected(id, &api).await?;

                transmit.send(event).await.ok();
            },
            ProtocolEvent::Disconnecting(id) => {
                transmit.send(Event::PeerDisconnected(id)).await.ok();
            },
            ProtocolEvent::Listening(addr) => {
                transmit.send(Event::Listening(addr)).await.ok();
            },
            ProtocolEvent::Membership(_) => {},
        }
        Ok(())
    }

    /// Attempt to track a project.
    async fn track_project(
        api: &PeerApi,
        urn: &Urn,
        peer_info: &PeerInfo<std::net::IpAddr>,
    ) -> Result<(), Error> {
        let peer_id = peer_info.peer_id;
        let port = peer_info.advertised_info.listen_port;
        let addr_hints = peer_info
            .seen_addrs
            .iter()
            .map(|a: &std::net::IpAddr| (*a, port).into())
            .collect::<Vec<_>>();
        let limit = api.fetch_limit();

        let result = {
            let urn = urn.clone();
            api.with_storage(move |storage| {
                replication::replicate(&storage, None, urn.clone(), peer_id, addr_hints, limit)?;
                tracking::track(&storage, &urn, peer_id)?;

                Ok::<_, Error>(())
            })
            .await?
        };

        match &result {
            Ok(()) => {
                tracing::info!("Successfully tracked project {} from peer {}", urn, peer_id);
            },
            Err(err) => {
                tracing::info!(
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
    async fn initialize_tracker(
        mode: &Mode,
        api: &PeerApi,
        transmit: &mut chan::Sender<Event>,
    ) -> Result<(), Error> {
        // Start by tracking specified projects if we need to.
        match &mode {
            Mode::TrackUrns(urns) => {
                tracing::info!("Initializing tracker with {} URNs..", urns.len());

                for urn in urns {
                    let mut peers = api.providers(urn.clone(), Duration::from_secs(30)).await;
                    // Attempt to track until we succeed.
                    while let Some(peer) = peers.next().await {
                        if Node::track_project(&api, urn, &peer).await.is_ok() {
                            let event =
                                Event::project_tracked(urn.clone(), peer.peer_id, api).await?;
                            transmit.send(event).await.ok();

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

/// Guess a user given a peer id.
async fn guess_user(peer: PeerId, api: &PeerApi) -> Result<Option<Person>, Error> {
    api.with_storage(move |s| {
        let users = identities::any::list(&s)?.filter_map(|res| {
            res.map(|id| match id {
                SomeIdentity::Person(user) => Some(user),
                _ => None,
            })
            .transpose()
        });

        for user in users {
            let user = user?;
            if user.delegations().contains(&peer) {
                return Ok(Some(user));
            }
        }

        Ok(None)
    })
    .await?
}
