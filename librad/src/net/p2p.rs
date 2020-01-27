use std::{
    collections::HashMap,
    error::Error,
    future::Future,
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use futures::{
    channel::{mpsc, oneshot, oneshot::Canceled},
    prelude::*,
};
use log::info;

use libp2p::{
    self,
    core::{muxing::StreamMuxerBox, nodes::Substream, transport::boxed::Boxed, upgrade},
    dns,
    noise,
    swarm::NetworkBehaviour,
    tcp,
    yamux,
    InboundUpgradeExt,
    Multiaddr,
    OutboundUpgradeExt,
    PeerId,
    Transport,
};

use crate::{
    keys::device,
    net::behaviour::{self, Behaviour},
    project::ProjectId,
};

pub use crate::net::protocol::{Capabilities, Capability, PeerInfo};

enum ToWorker {
    Have(ProjectId),
    GetProviders(ProjectId, oneshot::Sender<Vec<Provider>>),
    GetPeerInfo(PeerId, oneshot::Sender<PeerInfo>),
}

#[derive(Debug, Clone)]
pub struct Provider {
    pub project: ProjectId,
    pub peer: PeerId,
    pub addrs: Vec<Multiaddr>,
}

pub struct Service {
    to_worker: mpsc::UnboundedSender<ToWorker>,
}

impl Service {
    /// Announce that we have project [`ProjectId`]
    pub fn have(&self, pid: &ProjectId) {
        let _ = self.to_worker.unbounded_send(ToWorker::Have(pid.clone()));
    }

    /// Try to find peers providing project [`ProjectId`]
    pub fn providers(
        &self,
        pid: &ProjectId,
    ) -> impl Future<Output = Result<Vec<Provider>, Canceled>> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .to_worker
            .unbounded_send(ToWorker::GetProviders(pid.clone(), tx));
        rx
    }

    /// Try to determine the [`PeerInfo`] of peer [`PeerId`]
    pub fn peer_info(&self, peer: &PeerId) -> impl Future<Output = Result<PeerInfo, Canceled>> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .to_worker
            .unbounded_send(ToWorker::GetPeerInfo(peer.clone(), tx));
        rx
    }
}

type Swarm<S> = libp2p::swarm::Swarm<Boxed<(PeerId, StreamMuxerBox), io::Error>, Behaviour<S>>;

struct Subscribers {
    providers: HashMap<ProjectId, Vec<oneshot::Sender<Vec<Provider>>>>,
    peer_info: HashMap<PeerId, Vec<oneshot::Sender<PeerInfo>>>,
}

impl Default for Subscribers {
    fn default() -> Self {
        Self {
            providers: HashMap::default(),
            peer_info: HashMap::default(),
        }
    }
}

pub struct Worker {
    listening: bool,
    swarm: Swarm<Substream<StreamMuxerBox>>,
    service: Arc<Service>,
    from_service: mpsc::UnboundedReceiver<ToWorker>,
    subscribers: Subscribers,
}

impl Worker {
    pub fn new(
        key: device::Key,
        listen_addr: Option<Multiaddr>,
        capabilities: Capabilities,
    ) -> Result<Self, Box<dyn Error>> {
        let keypair = key.into_libp2p()?;
        let peer_id = PeerId::from(keypair.public());

        let mut swarm = {
            let transport = build_transport(keypair)?;
            let behaviour = Behaviour::new(&peer_id, capabilities)?;
            libp2p::Swarm::new(transport, behaviour, peer_id)
        };

        Swarm::listen_on(
            &mut swarm,
            listen_addr.unwrap_or_else(|| "/ip4/0.0.0.0/tcp/0".parse().unwrap()),
        )?;

        let (tx, rx) = mpsc::unbounded();
        let service = Arc::new(Service { to_worker: tx });
        Ok(Self {
            listening: false,
            swarm,
            service,
            from_service: rx,
            subscribers: Subscribers::default(),
        })
    }

    pub fn service(&self) -> &Arc<Service> {
        &self.service
    }
}

impl Future for Worker {
    type Output = Result<(), io::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        // See if we've got downstream events to process
        loop {
            let msg = match self.from_service.poll_next_unpin(cx) {
                Poll::Ready(Some(msg)) => msg,
                Poll::Ready(None) => return Poll::Ready(Ok(())),
                Poll::Pending => break,
            };

            match msg {
                ToWorker::Have(pid) => self.swarm.start_providing(&pid),
                ToWorker::GetProviders(pid, tx) => {
                    self.swarm.get_providers(&pid);
                    self.subscribers
                        .providers
                        .entry(pid)
                        .or_insert_with(Vec::new)
                        .push(tx);
                },
                ToWorker::GetPeerInfo(peer, tx) => {
                    self.swarm.get_peer_info(&peer);
                    self.subscribers
                        .peer_info
                        .entry(peer)
                        .or_insert_with(Vec::new)
                        .push(tx);
                },
            }
        }

        // See if we've got stuff on the network to process
        loop {
            match self.swarm.poll_next_unpin(cx) {
                Poll::Ready(None) => return Poll::Ready(Ok(())),
                Poll::Pending => {
                    // Output where we're listening, in case no listen_addr was
                    // given.
                    if !self.listening {
                        let listener = Swarm::listeners(&self.swarm).next();
                        if let Some(ref a) = listener {
                            info!("Listening on {:?}", a);
                            self.listening = true;
                        }
                    }
                    break;
                },
                Poll::Ready(Some(evt)) => match evt {
                    behaviour::Event::Provides { project, peers } => {
                        let providers: Vec<Provider> = peers
                            .iter()
                            .map(|peer_id| Provider {
                                project: project.clone(),
                                peer: peer_id.clone(),
                                addrs: self.swarm.addresses_of_peer(peer_id),
                            })
                            .collect();

                        if let Some(subscribers) = self.subscribers.providers.remove(&project) {
                            for tx in subscribers {
                                let _ = tx.send(providers.clone());
                            }
                        }
                    },

                    behaviour::Event::PeerInfo { peer_id, info } => {
                        if let Some(subscribers) = self.subscribers.peer_info.remove(&peer_id) {
                            for tx in subscribers {
                                let _ = tx.send(info.clone());
                            }
                        }
                    },
                },
            }
        }

        Poll::Pending
    }
}

fn build_transport(
    keypair: libp2p::identity::Keypair,
) -> Result<Boxed<(PeerId, StreamMuxerBox), io::Error>, io::Error> {
    let noise_config = {
        let noise_keypair = noise::Keypair::new()
            .into_authentic(&keypair)
            .expect("Initialising Noise keypair failed. This should never happen.");
        noise::NoiseConfig::ix(noise_keypair)
    };

    let transport = dns::DnsConfig::new(tcp::TcpConfig::new().nodelay(true))?;

    // Authentication (Noise)
    let transport = transport.and_then(move |stream, endpoint| {
        upgrade::apply(stream, noise_config, endpoint, upgrade::Version::V1).map(|out| match out? {
            (remote_id, out) => match remote_id {
                noise::RemoteIdentity::IdentityKey(key) => Ok((out, key.into_peer_id())),
                _ => Err(upgrade::UpgradeError::Apply(noise::NoiseError::InvalidKey)),
            },
        })
    });

    // Multiplexing
    let transport = transport.and_then(move |(stream, peer_id), endpoint| {
        let peer_id2 = peer_id.clone();
        let upgrade = yamux::Config::default()
            .map_inbound(move |muxer| (peer_id, muxer))
            .map_outbound(move |muxer| (peer_id2, muxer));

        upgrade::apply(stream, upgrade, endpoint, upgrade::Version::V1)
            .map_ok(|(id, muxer)| (id, StreamMuxerBox::new(muxer)))
    });

    let transport = transport
        .timeout(Duration::from_secs(20))
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
        .boxed();

    Ok(transport)
}
