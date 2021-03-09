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

use std::{collections::HashSet, net::SocketAddr, path::PathBuf, time::Duration};

use futures::{channel::mpsc as chan, select, sink::SinkExt as _, stream::StreamExt as _};
use thiserror::Error;

use librad::{
    git::{
        identities::{self, Urn},
        replication,
        tracking,
    },
    net::{
        discovery::{self, Discovery as _},
        peer::{self, Peer, ProtocolEvent},
        protocol::{self, PeerInfo},
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
    Storage(#[from] peer::StorageError),

    #[error(transparent)]
    Tracking(#[from] tracking::Error),

    #[error(transparent)]
    Identities(#[from] Box<identities::Error>),

    #[error(transparent)]
    Replication(#[from] replication::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Node(#[from] NodeError),

    #[error(transparent)]
    Channel(#[from] chan::SendError),

    #[error(transparent)]
    Profile(#[from] profile::Error),
}

impl From<identities::Error> for Error {
    fn from(e: identities::Error) -> Self {
        Self::Identities(Box::new(e))
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
    /// List of bootstrap peers
    pub bootstrap: Vec<(PeerId, SocketAddr)>,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            listen_addr: ([0, 0, 0, 0], 0).into(),
            mode: Mode::TrackEverything,
            root: None,
            network: Network::default(),
            bootstrap: vec![],
        }
    }
}

/// Seed node instance.
pub struct Node {
    /// Node configuration.
    config: NodeConfig,
    /// Peer configuration.
    peer_config: peer::Config<Signer>,
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
        let peer_config = peer::Config {
            signer,
            protocol: protocol::Config {
                paths,
                listen_addr: config.listen_addr,
                membership: Default::default(),
                network: config.network,
                replication: Default::default(),
            },
            storage_pools: Default::default(),
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
        let peer = Peer::new(self.peer_config);
        let mut events = peer.subscribe().boxed().fuse();
        let mut requests = self.requests;
        let mode = &self.config.mode;

        // Spawn the background peer thread.
        tokio::spawn({
            let peer = peer.clone();
            let disco = discovery::Static::resolve(self.config.bootstrap.clone())?;
            async move {
                loop {
                    match peer.bind().await {
                        Ok(bound) => {
                            if let Err(e) = bound.accept(disco.clone().discover()).await {
                                tracing::error!(err = ?e, "Accept error")
                            }
                        },
                        Err(e) => {
                            tracing::error!(err = ?e, "Bind error");
                            tokio::time::sleep(Duration::from_secs(2)).await
                        },
                    }
                }
            }
        });

        // Track already-known URNs.
        Node::initialize_tracker(mode, &peer, &mut transmit).await?;

        loop {
            select! {
                event = events.next() => {
                    match event {
                        Some(Ok(evt)) => {
                            Node::handle_event(evt, mode, &mut transmit, &peer).await?;
                        },
                        // There might be intermittent errors due to restarting.
                        // We can just ignore them.
                        Some(Err(_)) => {
                            continue;
                        },
                        // We're done when you're done.
                        None => {
                            break;
                        }
                    }
                }
                request = requests.next() => {
                    if let Some(r) = request {
                        Node::handle_request(r, &peer).await?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Handle user requests.
    async fn handle_request(request: Request, api: &Peer<Signer>) -> Result<(), Error> {
        match request {
            Request::GetMembership(mut reply) => {
                let info = api.membership().await;
                reply.send(info).await?;
            },
            Request::GetPeers(mut reply) => {
                let peers = api.connected_peers().await;
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
        event: ProtocolEvent,
        mode: &Mode,
        transmit: &mut chan::Sender<Event>,
        api: &Peer<Signer>,
    ) -> Result<(), Error> {
        use protocol::{
            broadcast::PutResult::Uninteresting,
            event::upstream::{Endpoint, Gossip},
        };

        match event {
            ProtocolEvent::Gossip(bx) => {
                let Gossip::Put {
                    provider,
                    payload,
                    result,
                } = bx.as_ref();

                // Only if the gossip message was considered uninteresting, is
                // it interesting: we are not yet tracking the peer / URN
                if *result == Uninteresting {
                    let urn = &payload.urn;
                    let peer_id = &provider.peer_id;

                    tracing::info!("Discovered new URN {} from peer {}", urn, peer_id);

                    if mode.is_trackable(peer_id, urn) {
                        // Attempt to track, but keep going if it fails.
                        if Node::track_project(&api, urn, &provider).await.is_ok() {
                            let event = Event::project_tracked(urn.clone(), *peer_id, &api).await?;
                            transmit.send(event).await.ok();
                        }
                    }
                }
            },
            ProtocolEvent::Endpoint(e) => {
                let event = match e {
                    Endpoint::Up { listen_addrs } => Event::Listening(listen_addrs),
                    Endpoint::Down => Event::Disconnected,
                };
                transmit.send(event).await.ok();
            },
            ProtocolEvent::Membership(_) => {},
        }
        Ok(())
    }

    /// Attempt to track a project.
    async fn track_project(
        api: &Peer<Signer>,
        urn: &Urn,
        peer_info: &PeerInfo<std::net::SocketAddr>,
    ) -> Result<(), Error> {
        let peer_id = peer_info.peer_id;
        let addr_hints = peer_info.seen_addrs.iter().copied().collect::<Vec<_>>();

        let result = {
            let cfg = api.protocol_config().replication;
            let urn = urn.clone();
            api.using_storage(move |storage| {
                replication::replicate(&storage, cfg, None, urn.clone(), peer_id, addr_hints)?;
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
        api: &Peer<Signer>,
        transmit: &mut chan::Sender<Event>,
    ) -> Result<(), Error> {
        // Start by tracking specified projects if we need to.
        match &mode {
            Mode::TrackUrns(urns) => {
                tracing::info!("Initializing tracker with {} URNs..", urns.len());

                for urn in urns {
                    let mut peers = api.providers(urn.clone(), Duration::from_secs(30));
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
