use std::{
    error::Error,
    marker::Unpin,
    task::{Context, Poll},
};

use async_std::task;
use futures::prelude::*;
use log::{debug, warn};

use libp2p::{
    self,
    kad::{self, record::store::MemoryStore, Kademlia, KademliaEvent},
    mdns::{Mdns, MdnsEvent},
    swarm::{NetworkBehaviourAction, NetworkBehaviourEventProcess},
    NetworkBehaviour,
    PeerId,
};

use crate::{net::protocol::Link, project::ProjectId};

pub enum Event {
    Provides {
        project: ProjectId,
        peers: Vec<PeerId>,
    },
}

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "Event", poll_method = "poll")]
pub struct Behaviour<S> {
    kademlia: Kademlia<S, MemoryStore>,
    mdns: Mdns<S>,
    link: Link,

    #[behaviour(ignore)]
    events: Vec<Event>,
}

impl<S> Behaviour<S> {
    pub fn new(peer_id: &PeerId) -> Result<Self, Box<dyn Error>> {
        let store = MemoryStore::new(peer_id.clone());
        let kademlia = Kademlia::new(peer_id.clone(), store);
        let mdns = task::block_on(Mdns::new())?;
        let link = Link;
        let events = vec![];

        Ok(Self {
            kademlia,
            mdns,
            link,
            events,
        })
    }

    fn poll<T>(&mut self, _: &mut Context) -> Poll<NetworkBehaviourAction<T, Event>> {
        if !self.events.is_empty() {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(self.events.remove(0)));
        }

        Poll::Pending
    }

    pub fn start_providing(&mut self, project: &ProjectId) {
        self.kademlia
            .start_providing(kad::record::Key::new(project))
    }

    pub fn get_providers(&mut self, project: &ProjectId) {
        self.kademlia.get_providers(kad::record::Key::new(project))
    }
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

        if let KademliaEvent::GetProvidersResult(Ok(res)) = message {
            let project = ProjectId::from_bytes(&res.key.to_vec()).map_err(|e| e.to_string());
            match project {
                Err(e) => warn!("GetProvidersResult: Invalid `ProjectId`: {}", e),
                Ok(pid) => self.events.push(Event::Provides {
                    project: pid,
                    peers: res.closest_peers,
                }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_project_id_kad_key_roundtrip() {
        let pid = ProjectId::from_str("67e6bd81be337c69385da551d93fd89fd3967eee.git").unwrap();
        let key = Key::new(&pid);
        let pid2 = ProjectId::from_bytes(&key.to_vec()).unwrap();

        assert_eq!(pid, pid2)
    }
}
