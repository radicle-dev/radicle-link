use std::{
    collections::HashSet,
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

use crate::{
    net::protocol::{Capabilities, PeerInfo},
    project::ProjectId,
};

#[derive(Debug)]
pub enum Event {
    Provides {
        project: ProjectId,
        peers: Vec<PeerId>,
    },

    PeerInfo {
        peer_id: PeerId,
        info: PeerInfo,
    },
}

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "Event", poll_method = "poll")]
pub struct Behaviour<S> {
    kademlia: Kademlia<S, MemoryStore>,
    mdns: Mdns<S>,

    #[behaviour(ignore)]
    local_peer_id: PeerId,

    #[behaviour(ignore)]
    local_peer_info: PeerInfo,

    #[behaviour(ignore)]
    events: Vec<Event>,
}

impl<S> Behaviour<S> {
    pub fn new(peer_id: &PeerId, capabilities: Capabilities) -> Result<Self, Box<dyn Error>> {
        let store = MemoryStore::new(peer_id.clone());
        let kademlia = Kademlia::new(peer_id.clone(), store);
        let mdns = Mdns::new()?;

        let mut moi = Self {
            kademlia,
            mdns,
            local_peer_id: peer_id.clone(),
            local_peer_info: PeerInfo {
                provided_projects: HashSet::default(),
                capabilities,
            },
            events: vec![],
        };

        moi.put_peer_info();

        Ok(moi)
    }

    fn poll<T>(&mut self, _: &mut Context) -> Poll<NetworkBehaviourAction<T, Event>> {
        if !self.events.is_empty() {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(self.events.remove(0)));
        }

        Poll::Pending
    }

    pub fn start_providing(&mut self, project: &ProjectId) {
        self.local_peer_info
            .provided_projects
            .insert(project.clone());
        self.put_peer_info();
        self.kademlia
            .start_providing(kad::record::Key::new(project));
    }

    pub fn get_providers(&mut self, project: &ProjectId) {
        self.kademlia.get_providers(kad::record::Key::new(project))
    }

    pub fn get_peer_info(&mut self, peer: &PeerId) {
        self.kademlia
            .get_record(&kad::record::Key::new(&format!("{}", peer)), Quorum::One)
    }

    fn put_peer_info(&mut self) {
        // TODO: sign
        self.kademlia.put_record(
            kad::record::Record {
                key: kad::record::Key::new(&format!("{}", self.local_peer_id)),
                value: serde_cbor::to_vec(&self.local_peer_info)
                    .expect("CBOR shallt not fail. qed"),
                publisher: Some(self.local_peer_id.clone()),
                expires: None,
            },
            Quorum::One,
        );
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
            },

            KademliaEvent::GetRecordResult(res) => match res {
                Err(e) => warn!("{:?}", e),
                Ok(recs) => {
                    for rec in recs.records {
                        let upstream_event: Result<Event, _> = {
                            PeerId::from_bytes(rec.key.to_vec())
                                .map_err(|_| format_err!("Record key is not a PeerId"))
                                .and_then(|peer_id| {
                                    serde_cbor::from_slice(&rec.value)
                                        .map_err(|e| format_err!("{}", e))
                                        .map(|info| Event::PeerInfo { peer_id, info })
                                })
                        };

                        match upstream_event {
                            Err(e) => warn!("GetRecordResult: {:?}", e),
                            Ok(evt) => self.events.push(evt),
                        }
                    }
                },
            },

            _ => {},
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
