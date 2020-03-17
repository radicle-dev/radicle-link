use serde::{Deserialize, Serialize};

use crate::{
    net::gossip::types::{PeerAdvertisement, PeerInfo},
    project::ProjectId,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Rpc {
    Membership(Membership),
    Gossip(Gossip),
}

impl From<Membership> for Rpc {
    fn from(m: Membership) -> Self {
        Self::Membership(m)
    }
}

impl From<Gossip> for Rpc {
    fn from(g: Gossip) -> Self {
        Self::Gossip(g)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Membership {
    Join(PeerAdvertisement),
    ForwardJoin {
        joined: PeerInfo,
        ttl: usize,
    },
    Neighbour(PeerAdvertisement),
    Shuffle {
        origin: PeerInfo,
        peers: Vec<PeerInfo>,
        ttl: usize,
    },
    ShuffleReply {
        peers: Vec<PeerInfo>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Gossip {
    Have { origin: PeerInfo, val: Update },
    Want { origin: PeerInfo, val: Update },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Update {
    Project {
        project: ProjectId,
        head: Option<Ref>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ref {
    name: String,
    target: Vec<u8>,
}
