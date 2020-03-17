use std::{collections::HashSet, net::SocketAddr};

use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

use crate::peer::PeerId;

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum Capability {
    Reserved = 0,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerInfo {
    pub peer_id: PeerId,
    pub advertised_info: PeerAdvertisement,
    pub seen_addrs: HashSet<SocketAddr>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerAdvertisement {
    pub listen_addr: SocketAddr,
    pub capabilities: HashSet<Capability>,
}

impl PeerAdvertisement {
    pub fn new(listen_addr: SocketAddr) -> Self {
        Self {
            listen_addr,
            capabilities: HashSet::default(),
        }
    }
}
