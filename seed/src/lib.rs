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

pub mod event;
pub mod handle;
pub mod project;
pub mod signer;

use std::{collections::HashSet, net::SocketAddr, path::PathBuf, time::Duration, vec};

use futures::{channel::mpsc as chan, select, sink::SinkExt as _, stream::StreamExt as _};
use thiserror::Error;

use librad::{
    git,
    keys,
    meta::{self, entity, user::User},
    net::{
        discovery,
        gossip,
        gossip::types::PeerInfo,
        peer::{self, PeerApi, PeerConfig},
        protocol::ProtocolEvent,
    },
    paths,
    peer::PeerId,
    uri::{self, RadUrn},
};

pub use crate::{
    event::Event,
    handle::{NodeError, NodeHandle, Request},
    project::Project,
    signer::Signer,
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
    Git(#[from] git::repo::Error),

    #[error(transparent)]
    Bootstrap(#[from] peer::BootstrapError),

    #[error(transparent)]
    Accept(#[from] peer::AcceptError),

    #[error(transparent)]
    Node(#[from] NodeError),

    #[error(transparent)]
    Channel(#[from] chan::SendError),
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
    /// Receiver end of user requests.
    requests: chan::UnboundedReceiver<Request>,
    /// Sender end of user requests.
    handle: chan::UnboundedSender<Request>,
}

impl Node {
    /// Create a new seed node.
    pub fn new(config: NodeConfig) -> Result<Self, Error> {
        let (handle, requests) = chan::unbounded::<Request>();

        Ok(Node {
            config,
            handle,
            requests,
        })
    }

    /// Create a new handle.
    pub fn handle(&self) -> NodeHandle {
        NodeHandle::new(self.handle.clone())
    }

    /// Run the seed node. This function runs indefinitely until a fatal error
    /// occurs.
    pub async fn run(self, mut transmit: chan::Sender<Event>) -> Result<(), Error> {
        let paths = if let Some(root) = &self.config.root {
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
    async fn handle_request(request: Request, api: &PeerApi<Signer>) -> Result<(), Error> {
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
        api: &PeerApi<Signer>,
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
        api: &PeerApi<Signer>,
        urn: &RadUrn,
        peer_info: &PeerInfo<std::net::IpAddr>,
    ) -> Result<(), Error> {
        let peer_id = peer_info.peer_id;
        let url = urn.clone().into_rad_url(peer_id);
        let project_urn = RadUrn {
            path: uri::Path::new(),
            ..urn.clone()
        };
        let port = peer_info.advertised_info.listen_port;
        let addr_hints = peer_info
            .seen_addrs
            .iter()
            .map(|a: &std::net::IpAddr| (*a, port).into())
            .collect::<Vec<_>>();

        // Track unconditionally.
        {
            let urn = urn.clone();
            api.with_storage(move |storage| storage.track(&urn, &peer_id))
                .await??
        }

        let result = {
            let urn = project_urn.clone();
            api.with_storage(move |storage| -> Result<(), librad::git::storage::Error> {
                // FIXME(xla): There should be a saner way to test.
                let exists = storage.has_urn(&urn)?;

                if exists {
                    storage.fetch_repo(url, addr_hints)
                } else {
                    storage
                        .clone_repo::<meta::project::ProjectInfo, _>(url, addr_hints)
                        .map(|_info| ())
                }
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
        api: &PeerApi<Signer>,
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
async fn guess_user(
    peer: PeerId,
    api: &PeerApi<Signer>,
) -> Result<Option<User<entity::Draft>>, Error> {
    api.with_storage(move |s| {
        let metas = s.all_metadata()?;

        for meta in metas {
            let meta = meta?;
            let repo = s.open_repo(meta.urn())?;

            for remote in repo.tracked()? {
                if remote == peer {
                    let user = repo.get_rad_self_of(remote)?;

                    return Ok(Some(user));
                }
            }
        }
        Ok(None)
    })
    .await?
}
