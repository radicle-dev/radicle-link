use std::{collections::HashMap, error::Error, net::SocketAddr};

use quinn::{self, NewConnection};

use crate::{
    net::connection::{CloseReason, Connection},
    peer::PeerId,
};

#[derive(Clone)]
pub struct Membership {
    endpoint: quinn::Endpoint,
    connected_peers: HashMap<PeerId, Connection>,
}

impl Membership {
    pub fn new(endpoint: quinn::Endpoint) -> Self {
        Self {
            endpoint,
            connected_peers: Default::default(),
        }
    }

    pub fn connected_peers(&self) -> impl Iterator<Item = &Connection> {
        self.connected_peers.values()
    }

    pub async fn connect(
        &mut self,
        peer: &PeerId,
        addr: &SocketAddr,
    ) -> Result<(Connection, quinn::IncomingBiStreams), Box<dyn Error>> {
        let NewConnection {
            connection,
            bi_streams,
            ..
        } = self
            .endpoint
            .connect(addr, &format!("{}.radicle", peer))?
            .await?;

        let conn = Connection::new(peer, connection);

        if let Some(old) = self.connected_peers.insert(peer.clone(), conn.clone()) {
            old.close(CloseReason::DuplicateConnection)
        }

        Ok((conn, bi_streams))
    }

    pub fn get_connection(&self, peer: &PeerId) -> Option<Connection> {
        self.connected_peers.get(peer).cloned()
    }

    pub fn disconnect(&mut self, peer: &PeerId) {
        if let Some(conn) = self.connected_peers.remove(peer) {
            conn.close(CloseReason::ProtocolDisconnect)
        }
    }
}
