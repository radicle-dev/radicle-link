use std::time::Duration;
use std::{
    error::Error,
    task::{Context, Poll},
};

use log::{debug, info};

use async_std::{io, task};
use futures::prelude::*;

use libp2p::kad::record::store::MemoryStore;
use libp2p::kad::{Kademlia, KademliaEvent};
use libp2p::{
    self, dns,
    mdns::{Mdns, MdnsEvent},
    mplex, secio,
    swarm::NetworkBehaviourEventProcess,
    tcp, yamux, Multiaddr, NetworkBehaviour, PeerId, Swarm, Transport, TransportError,
};

use crate::keys::device;

#[derive(NetworkBehaviour)]
pub struct RadBehaviour<TSubstream> {
    kademlia: Kademlia<TSubstream, MemoryStore>,
    mdns: Mdns<TSubstream>,
}

impl<TSubstream: AsyncRead + AsyncWrite> NetworkBehaviourEventProcess<MdnsEvent>
    for RadBehaviour<TSubstream>
{
    fn inject_event(&mut self, event: MdnsEvent) {
        if let MdnsEvent::Discovered(list) = event {
            for (peer_id, multiaddr) in list {
                debug!("Discovered peer via mDNS: {} @ {}", peer_id, multiaddr);
                self.kademlia.add_address(&peer_id, multiaddr);
            }
        }
    }
}

impl<TSubstream: AsyncRead + AsyncWrite> NetworkBehaviourEventProcess<KademliaEvent>
    for RadBehaviour<TSubstream>
{
    // Called when `kademlia` produces an event.
    fn inject_event(&mut self, message: KademliaEvent) {
        debug!("Received KademliaEvent: {:?}", message);
    }
}

pub async fn join(key: device::Key, listen_addr: Option<Multiaddr>) -> Result<(), Box<dyn Error>> {
    let key = key.into_libp2p()?;
    let peer_id = PeerId::from(key.public());

    let transport = RadTransport::new()?.establish(key);
    let mut swarm = {
        let store = MemoryStore::new(peer_id.clone());
        let kademlia = Kademlia::new(peer_id.clone(), store);
        let mdns = task::block_on(Mdns::new())?;

        let behaviour = RadBehaviour { kademlia, mdns };

        Swarm::new(transport, behaviour, peer_id)
    };

    Swarm::listen_on(
        &mut swarm,
        listen_addr.unwrap_or_else(|| "/ip4/0.0.0.0/tcp/0".parse().unwrap()),
    )?;

    let mut listening = false;
    task::block_on(future::poll_fn(move |cx: &mut Context| {
        loop {
            match swarm.poll_next_unpin(cx) {
                Poll::Ready(Some(x)) => debug!("swarm.poll returned something: {:?}", x),
                Poll::Ready(None) => return Poll::Ready(Ok(())),
                Poll::Pending => {
                    if !listening {
                        if let Some(a) = Swarm::listeners(&swarm).next() {
                            info!("Listening on {:?}", a);
                            listening = true;
                        }
                    }
                    break;
                }
            }
        }

        Poll::Pending
    }));

    Ok(())
}

type TransportImpl = dns::DnsConfig<tcp::TcpConfig>;

#[derive(Clone)]
pub struct RadTransport(TransportImpl);

impl RadTransport {
    pub fn new() -> Result<Self, io::Error> {
        dns::DnsConfig::new(tcp::TcpConfig::new().nodelay(true)).map(Self)
    }

    pub fn establish(
        self,
        keypair: libp2p::identity::Keypair,
    ) -> impl Transport<
        Output = (
            PeerId,
            impl libp2p::core::muxing::StreamMuxer<
                    OutboundSubstream = impl Send,
                    Substream = impl Send,
                    Error = impl Into<io::Error>,
                > + Send
                + Sync,
        ),
        Error = impl std::error::Error + Send,
        Listener = impl Send,
        Dial = impl Send,
        ListenerUpgrade = impl Send,
    > + Clone {
        self.upgrade(libp2p::core::upgrade::Version::V1)
            .authenticate(secio::SecioConfig::new(keypair))
            .multiplex(libp2p::core::upgrade::SelectUpgrade::new(
                yamux::Config::default(),
                mplex::MplexConfig::new(),
            ))
            .map(|(peer, muxer), _| (peer, libp2p::core::muxing::StreamMuxerBox::new(muxer)))
            .timeout(Duration::from_secs(20))
    }
}

impl Transport for RadTransport {
    type Output = <TransportImpl as Transport>::Output;
    type Error = <TransportImpl as Transport>::Error;
    type Listener = <TransportImpl as Transport>::Listener;
    type ListenerUpgrade = <TransportImpl as Transport>::ListenerUpgrade;
    type Dial = <TransportImpl as Transport>::Dial;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        self.0.listen_on(addr)
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        self.0.dial(addr)
    }
}
