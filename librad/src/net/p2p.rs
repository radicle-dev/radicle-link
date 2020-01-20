use std::{
    error::Error,
    future::Future,
    io,
    marker::Unpin,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use async_std::task;
use futures::prelude::*;
use log::{debug, info};

use libp2p::{
    self,
    core::{muxing::StreamMuxerBox, nodes::Substream, transport::boxed::Boxed, upgrade},
    dns,
    kad::{
        record::{store::MemoryStore, Key},
        Kademlia,
        KademliaEvent,
    },
    mdns::{Mdns, MdnsEvent},
    noise,
    swarm::NetworkBehaviourEventProcess,
    tcp,
    yamux,
    InboundUpgradeExt,
    Multiaddr,
    NetworkBehaviour,
    OutboundUpgradeExt,
    PeerId,
    Transport,
};

use crate::{keys::device, project::ProjectId};

enum ToWorker {
    HaveProject(ProjectId),
}

pub struct Service {
    to_worker: futures::channel::mpsc::UnboundedSender<ToWorker>,
}

impl Service {
    pub fn have_project(&self, proj: ProjectId) {
        let _ = self.to_worker.unbounded_send(ToWorker::HaveProject(proj));
    }
}

type Swarm<S> = libp2p::swarm::Swarm<Boxed<(PeerId, StreamMuxerBox), io::Error>, Behaviour<S>>;

pub struct Worker {
    listening: bool,
    swarm: Swarm<Substream<StreamMuxerBox>>,
    service: Arc<Service>,
    from_service: futures::channel::mpsc::UnboundedReceiver<ToWorker>,
}

impl Worker {
    pub fn new(key: device::Key, listen_addr: Option<Multiaddr>) -> Result<Self, Box<dyn Error>> {
        let keypair = key.into_libp2p()?;
        let peer_id = PeerId::from(keypair.public());

        let mut swarm = {
            let transport = build_transport(keypair)?;
            let store = MemoryStore::new(peer_id.clone());
            let kademlia = Kademlia::new(peer_id.clone(), store);
            let mdns = task::block_on(Mdns::new())?;

            let behaviour = Behaviour { kademlia, mdns };
            libp2p::Swarm::new(transport, behaviour, peer_id)
        };

        Swarm::listen_on(
            &mut swarm,
            listen_addr.unwrap_or_else(|| "/ip4/0.0.0.0/tcp/0".parse().unwrap()),
        )?;

        let (tx, rx) = futures::channel::mpsc::unbounded();
        let service = Arc::new(Service { to_worker: tx });
        Ok(Self {
            listening: false,
            swarm,
            service,
            from_service: rx,
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
                ToWorker::HaveProject(pid) => self.swarm.kademlia.start_providing(Key::new(&pid)),
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
                }
                Poll::Ready(Some(x)) => debug!("Recv {:?}", x),
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

/*
enum UpstreamEvent {
    Providers(GetProvidersOk),
    NewPeer {
        peer_id: PeerId,
        addrs: Vec<Multiaddr>,
    },
}
*/

#[derive(NetworkBehaviour)]
pub struct Behaviour<S> {
    kademlia: Kademlia<S, MemoryStore>,
    mdns: Mdns<S>,
}

impl<S: AsyncRead + AsyncWrite> NetworkBehaviourEventProcess<MdnsEvent> for Behaviour<S> {
    fn inject_event(&mut self, event: MdnsEvent) {
        if let MdnsEvent::Discovered(list) = event {
            for (peer_id, addr) in list {
                debug!("Disovered peer via mDNS: {} @ {}", peer_id, addr);
                self.kademlia.add_address(&peer_id, addr);
            }
        }
    }
}

impl<S: AsyncRead + AsyncWrite> NetworkBehaviourEventProcess<KademliaEvent> for Behaviour<S> {
    // Called when `kademlia` produces an event.
    fn inject_event(&mut self, message: KademliaEvent) {
        debug!("Received KademliaEvent: {:?}", message);
    }
}
