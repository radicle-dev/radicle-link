// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#![allow(clippy::single_match)]
#[macro_use]
extern crate async_trait;

use std::{
    collections::HashSet,
    net::SocketAddr,
    panic,
    sync::{
        atomic::{AtomicBool, Ordering::SeqCst},
        Arc,
    },
    thread,
    time::Duration,
};

use futures::{future::FutureExt as _, pin_mut, select, stream::StreamExt as _};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use librad::{
    git::{
        identities::{self, Urn},
        replication,
        storage::fetcher,
        tracking,
    },
    net::{
        discovery::{self, Discovery as _},
        peer::{self, Peer, ProtocolEvent},
        protocol::{self, gossip::Payload, PeerInfo},
    },
    peer::PeerId,
    profile,
};

pub use crate::{
    event::Event,
    handle::{NodeError, NodeHandle, Request},
    project::Project,
    signer::Signer,
};

pub mod event;
pub mod handle;
pub mod project;
pub mod signer;

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
    #[error("unable to resolve URN {0}")]
    NoSuchUrn(Urn),

    #[error("error creating fetcher")]
    MkFetcher(#[from] Box<dyn std::error::Error + Send + Sync + 'static>),

    #[error("replication of {urn} from {remote_peer} already in-flight")]
    Concurrent { urn: Urn, remote_peer: PeerId },

    #[error(transparent)]
    InitPeer(#[from] peer::error::Init),

    #[error(transparent)]
    Storage(#[from] peer::error::Storage),

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
    Profile(#[from] profile::Error),

    #[error("sending reply failed for {0}")]
    Reply(String),

    #[error("failed to set up signal handler")]
    Signals,
}

impl From<identities::Error> for Error {
    fn from(e: identities::Error) -> Self {
        Self::Identities(Box::new(e))
    }
}

impl From<fetcher::Info> for Error {
    fn from(
        fetcher::Info {
            urn, remote_peer, ..
        }: fetcher::Info,
    ) -> Self {
        Self::Concurrent { urn, remote_peer }
    }
}

/// Seed operational mode.
#[derive(Clone, Debug)]
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
    /// List of bootstrap peers
    pub bootstrap: Vec<(PeerId, SocketAddr)>,
    /// Knobs to tune timeouts and internal queues.
    pub limits: Limits,
    /// Operational mode. Determines the tracking rules for this seed node.
    pub mode: Mode,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            bootstrap: vec![],
            limits: Default::default(),
            mode: Mode::TrackEverything,
        }
    }
}

pub struct Limits {
    /// Amount of in-flight requests.
    pub request_queue_size: usize,
    /// Duration after which a request is considered failed.
    pub request_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            request_queue_size: 64,
            request_timeout: Duration::from_secs(3),
        }
    }
}

/// Seed node instance.
pub struct Node {
    /// Config that was passed during Node construction.
    config: NodeConfig,
    /// Sender end of user requests.
    handle: mpsc::Sender<Request>,
    /// Receiver end of user requests.
    requests: mpsc::Receiver<Request>,
}

impl Node {
    /// Create a new seed node.
    pub fn new(config: NodeConfig) -> Result<Self, Error> {
        let (handle, requests) = mpsc::channel::<Request>(config.limits.request_queue_size);

        Ok(Node {
            config,
            handle,
            requests,
        })
    }

    /// Create a new handle.
    pub fn handle(&self) -> NodeHandle {
        NodeHandle::new(self.handle.clone(), self.config.limits.request_timeout)
    }

    /// Run the seed node. This function runs indefinitely until a fatal error
    /// occurs or a termination signal is sent to the process.
    pub async fn run(
        self,
        peer_config: peer::Config<Signer>,
        mut transmit: mpsc::Sender<Event>,
    ) -> Result<(), Error> {
        let peer = Peer::new(peer_config)?;
        let events = peer.subscribe().fuse();
        let mut requests = ReceiverStream::new(self.requests).fuse();

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let shutdown_rx = shutdown_rx.fuse();

        // Spawn the peer thread.
        let mut protocol = tokio::spawn({
            let peer = peer.clone();
            let disco = discovery::Static::resolve(self.config.bootstrap.clone())?;
            async move {
                futures::pin_mut!(shutdown_rx);
                loop {
                    match peer.bind().await {
                        Ok(bound) => {
                            let (stop_accepting, run) = bound.accept(disco.clone().discover());
                            let run = run.fuse();
                            futures::pin_mut!(run);
                            let result = futures::select! {
                                _ = shutdown_rx => {
                                    stop_accepting();
                                    run.await
                                }
                                result = run => result
                            };
                            match result {
                                Err(protocol::io::error::Accept::Done) => {
                                    tracing::info!("network endpoint shut down");
                                    return;
                                },
                                Err(error) => {
                                    tracing::error!(?error, "accept error");
                                },
                                Ok(never) => never,
                            };
                        },
                        Err(e) => {
                            tracing::error!(err = ?e, "Bind error");
                            let sleep = tokio::time::sleep(Duration::from_secs(2)).fuse();
                            futures::pin_mut!(sleep);
                            futures::select! {
                                _ = sleep => {},
                                _ = shutdown_rx => {
                                    return;
                                }
                            };
                        },
                    }
                }
            }
        })
        .fuse();
        // Set up signal handlers
        {
            use signal_hook::{
                consts::TERM_SIGNALS,
                flag::register_conditional_shutdown,
                low_level::register,
            };

            let stop = Arc::new(AtomicBool::new(false));
            let sig_handler = thread::spawn({
                let stop = Arc::clone(&stop);
                move || loop {
                    if stop.load(SeqCst) {
                        let _ = shutdown_tx.send(());
                        break;
                    }

                    thread::park()
                }
            });

            for sig in TERM_SIGNALS {
                register_conditional_shutdown(*sig, 1, Arc::clone(&stop))
                    .or(Err(Error::Signals))?;
                unsafe {
                    let stop = Arc::clone(&stop);
                    let thread = sig_handler.thread().clone();
                    register(*sig, move || {
                        stop.store(true, SeqCst);
                        thread.unpark()
                    })
                    .or(Err(Error::Signals))?;
                }
            }
        }

        // Track already-known URNs.
        Node::initialize_tracker(&self.config.mode, &peer, &mut transmit).await?;

        pin_mut!(events);
        loop {
            select! {
                p = protocol => match p {
                    Err(e) if e.is_panic() => panic::resume_unwind(e.into_panic()),
                    _ => break
                },

                event = events.next() => {
                    match event {
                        Some(Ok(evt)) => {
                            let mode = self.config.mode.clone();
                            let peer = peer.clone();
                            let mut transmit = transmit.clone();
                            tokio::spawn(async move {
                                if let Err(err) = Node::handle_event(evt, mode, &mut transmit, &peer).await {
                                    tracing::error!(err = ?err, "event fulfilment failed");
                                }
                            });
                        },
                        // There might be intermittent errors due to restarting.
                        // We can just ignore them.
                        Some(Err(err)) => {
                            tracing::error!(err = ?err, "event loop");
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
                        let peer = peer.clone();

                        tokio::spawn(async move {
                            if let Err(err) =  Node::handle_request(r, &peer).await {
                                tracing::error!(err = ?err, "request fulfilment failed");
                            }
                        });
                    }
                }
            }
        }

        Ok(())
    }

    /// Handle user requests.
    async fn handle_request(request: Request, api: &Peer<Signer>) -> Result<(), Error> {
        match request {
            Request::GetMembership(reply) => {
                let info = api.membership().await;
                reply
                    .send(info)
                    .map_err(|_| Error::Reply("GetMembership".to_string()))
            },
            Request::GetPeers(reply) => {
                let peers = api.connected_peers().await;
                reply
                    .send(peers)
                    .map_err(|_| Error::Reply("GetPeers".to_string()))
            },
            Request::GetProjects(reply) => {
                let projs = project::get_projects(api).await?;
                reply
                    .send(projs)
                    .map_err(|_| Error::Reply("GetProjects".to_string()))
            },
        }
    }

    /// Handle gossip events. As soon as a peer announces something of interest,
    /// we check if we should track it.
    async fn handle_event(
        event: ProtocolEvent,
        mode: Mode,
        transmit: &mut mpsc::Sender<Event>,
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
                        if Node::track_project(api, urn, provider).await.is_ok() {
                            let event = Event::project_tracked(urn.clone(), *peer_id, api).await?;
                            api.announce(Payload {
                                urn: urn.clone(),
                                rev: None,
                                origin: Some(*peer_id),
                            })
                            .ok();
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
            _ => {},
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
                let fetcher = fetcher::PeerToPeer::new(urn.clone(), peer_id, addr_hints)
                    .build(storage)
                    .map_err(|e| Error::MkFetcher(e.into()))??;
                replication::replicate(storage, fetcher, cfg, None)?;
                tracking::track(storage, &urn, peer_id)?;

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
        result
    }

    /// Attempt to track initial URN list, if any.
    async fn initialize_tracker(
        mode: &Mode,
        api: &Peer<Signer>,
        transmit: &mut mpsc::Sender<Event>,
    ) -> Result<(), Error> {
        // Start by tracking specified projects if we need to.
        match &mode {
            Mode::TrackUrns(urns) => {
                tracing::info!("Initializing tracker with {} URNs..", urns.len());

                for urn in urns {
                    let mut peers = api.providers(urn.clone(), Duration::from_secs(30));
                    // Attempt to track until we succeed.
                    while let Some(peer) = peers.next().await {
                        if Node::track_project(api, urn, &peer).await.is_ok() {
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
