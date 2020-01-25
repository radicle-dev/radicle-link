use std::{
    error::Error,
    marker::Unpin,
    task::{Context, Poll},
};

use failure::format_err;
use futures::prelude::*;
use log::{debug, warn};

use libp2p::{
    self,
    kad::{self, record::store::MemoryStore, Kademlia, KademliaEvent, Quorum},
    mdns::{Mdns, MdnsEvent},
    swarm::{NetworkBehaviourAction, NetworkBehaviourEventProcess},
    NetworkBehaviour,
    PeerId,
};

use crate::{net::protocol::Capabilities, project::ProjectId};

#[derive(Debug)]
pub enum Event {
    Provides {
        project: ProjectId,
        peers: Vec<PeerId>,
    },

    CapabilitiesOf {
        peer: PeerId,
        capabilities: Capabilities,
    },
}

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "Event", poll_method = "poll")]
pub struct Behaviour<S> {
    kademlia: Kademlia<S, MemoryStore>,
    mdns: Mdns<S>,

    #[behaviour(ignore)]
    events: Vec<Event>,
}

impl<S> Behaviour<S> {
    pub fn new(peer_id: &PeerId, capabilities: Capabilities) -> Result<Self, Box<dyn Error>> {
        let store = MemoryStore::new(peer_id.clone());
        let mut kademlia = Kademlia::new(peer_id.clone(), store);
        let mdns = Mdns::new()?;
        let events = vec![];

        // Abuse the DHT to publish some info about us.
        kademlia.put_record(
            kad::record::Record {
                key: Self::capabilities_key(&peer_id),
                value: serde_cbor::to_vec(&capabilities).expect("CBOR shallt not fail. qed"),
                publisher: Some(peer_id.clone()),
                expires: None,
            },
            Quorum::One,
        );

        Ok(Self {
            kademlia,
            mdns,
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

    pub fn get_capabilities(&mut self, peer: &PeerId) {
        self.kademlia
            .get_record(&Self::capabilities_key(peer), Quorum::One)
    }

    fn capabilities_key(peer_id: &PeerId) -> kad::record::Key {
        kad::record::Key::new(&format!("capsof-{}", peer_id))
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

        match message {
            KademliaEvent::GetProvidersResult(Ok(res)) => {
                let upstream_event =
                    ProjectId::from_bytes(&res.key.to_vec()).map(|pid| Event::Provides {
                        project: pid,
                        peers: res.closest_peers,
                    });

                match upstream_event {
                    Err(e) => warn!("GetProvidersResult: Invalid `ProjectId`: {}", e),
                    Ok(evt) => self.events.push(evt),
                }
            }

            KademliaEvent::GetRecordResult(Ok(res)) => {
                for record in res.records {
                    let upstream_event = || {
                        let mut key = String::from_utf8(record.key.to_vec())?;
                        if !key.starts_with("capsof-") {
                            Err(format_err!("Unexpected record key prefix: {}", key))
                        } else {
                            let peer = key.split_off(7).parse::<PeerId>()?;
                            let capabilities = serde_cbor::from_slice(&record.value)?;
                            Ok(Event::CapabilitiesOf { peer, capabilities })
                        }
                    };

                    let upstream_event = upstream_event();
                    debug!("{:?}", upstream_event);
                    match upstream_event {
                        Err(e) => warn!("{}", e),
                        Ok(evt) => {
                            debug!("caps event: {:?}", evt);
                            self.events.push(evt)
                        }
                    }
                }
            }

            _ => {}
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
        let key = kad::record::Key::new(&pid);
        let pid2 = ProjectId::from_bytes(&key.to_vec()).unwrap();

        assert_eq!(pid, pid2)
    }
}
