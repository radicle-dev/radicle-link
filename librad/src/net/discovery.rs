use std::net::{SocketAddr, ToSocketAddrs};

use crate::peer::PeerId;

pub trait Discovery {
    fn collect(&self) -> Vec<(PeerId, Vec<SocketAddr>)>;
}

pub struct Static<S: ToSocketAddrs> {
    peers: Vec<(PeerId, S)>,
}

impl<S: ToSocketAddrs> Static<S> {
    pub fn new<I: IntoIterator<Item = (PeerId, S)>>(peers: I) -> Self {
        Self {
            peers: peers.into_iter().collect(),
        }
    }
}

impl<S: ToSocketAddrs> Discovery for Static<S> {
    fn collect(&self) -> Vec<(PeerId, Vec<SocketAddr>)> {
        self.peers
            .iter()
            .filter_map(|(peer_id, addrs)| {
                addrs
                    .to_socket_addrs()
                    .ok()
                    .map(|resolved| (peer_id.clone(), resolved.collect()))
            })
            .collect()
    }
}
