use std::error::Error;

use futures::prelude::*;
use libp2p::kad::record::store::MemoryStore;
use libp2p::kad::{Kademlia, KademliaEvent};
use libp2p::{
    build_development_transport,
    mdns::{Mdns, MdnsEvent},
    swarm::NetworkBehaviourEventProcess,
    tokio_io::{AsyncRead, AsyncWrite},
    NetworkBehaviour, PeerId, Swarm,
};
use log::{debug, info};
use structopt::StructOpt;

use librad::keys::device;
use librad::paths::Paths;

#[derive(StructOpt)]
pub struct Options {}

pub fn run(_paths: Paths, _opts: Options, key: device::Key) -> Result<(), Box<dyn Error>> {
    let local_key = key.into_libp2p()?;
    let local_peer_id = PeerId::from(local_key.public());

    info!("My Peer Id: {}", local_peer_id);

    let transport = build_development_transport(local_key);

    // We create a custom network behaviour that combines Kademlia and mDNS.
    #[derive(NetworkBehaviour)]
    struct MyBehaviour<TSubstream: AsyncRead + AsyncWrite> {
        kademlia: Kademlia<TSubstream, MemoryStore>,
        mdns: Mdns<TSubstream>,
    }

    impl<TSubstream: AsyncRead + AsyncWrite> NetworkBehaviourEventProcess<MdnsEvent>
        for MyBehaviour<TSubstream>
    {
        // Called when `mdns` produces an event.
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
        for MyBehaviour<TSubstream>
    {
        // Called when `kademlia` produces an event.
        fn inject_event(&mut self, message: KademliaEvent) {
            info!("received KademliaEvent: {:?}", message);
        }
    }

    // Create a swarm to manage peers and events.
    let mut swarm = {
        // Create a Kademlia behaviour.
        let store = MemoryStore::new(local_peer_id.clone());
        let kademlia = Kademlia::new(local_peer_id.clone(), store);

        let behaviour = MyBehaviour {
            kademlia,
            mdns: Mdns::new().expect("Failed to create mDNS service"),
        };

        Swarm::new(transport, behaviour, local_peer_id.clone())
    };

    // Listen on all interfaces and whatever port the OS assigns.
    Swarm::listen_on(&mut swarm, "/ip4/0.0.0.0/tcp/0".parse().unwrap()).unwrap();

    // Kick it off.
    let mut listening = false;
    tokio::run(futures::future::poll_fn(move || {
        loop {
            debug!("polling swarm");
            match swarm.poll().expect("Error while polling swarm") {
                Async::Ready(Some(x)) => debug!("swarm.poll returned something: {:?}", x),
                Async::Ready(None) | Async::NotReady => {
                    debug!("Ready(None) or NotReady");
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

        Ok(Async::NotReady)
    }));

    Ok(())
}
