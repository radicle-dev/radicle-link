use std::{collections::HashMap, error::Error, net::SocketAddr};

use futures::{AsyncRead, AsyncWrite};
use quinn::{self, NewConnection, VarInt};

use crate::peer::PeerId;

#[derive(Clone)]
pub struct Connection {
    peer: PeerId,
    conn: quinn::Connection,
}

impl Connection {
    pub fn peer_id(&self) -> &PeerId {
        &self.peer
    }

    pub fn remote_address(&self) -> SocketAddr {
        self.conn.remote_address()
    }

    pub async fn open_stream(&self) -> Result<(impl AsyncRead, impl AsyncWrite), Box<dyn Error>> {
        let (send, recv) = self.conn.open_bi().await?;
        Ok((recv, send))
    }
}

pub const ERRNO_CLOSE_DUP: VarInt = VarInt::from_u32(1);
pub const REASON_CLOSE_DUP: &[u8] = b"duplicate connection";

pub const ERRNO_CLOSE_USR: VarInt = VarInt::from_u32(2);

pub struct Membership {
    endpoint: quinn::Endpoint,
    connected_peers: HashMap<PeerId, Connection>,
}

impl Membership {
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

        let conn = Connection {
            peer: peer.clone(),
            conn: connection,
        };

        if let Some(old) = self.connected_peers.insert(peer.clone(), conn.clone()) {
            old.conn.close(ERRNO_CLOSE_DUP, REASON_CLOSE_DUP);
        }

        Ok((conn, bi_streams))
    }

    pub fn get_connection(&self, peer: &PeerId) -> Option<Connection> {
        self.connected_peers.get(peer).cloned()
    }

    pub fn disconnect(&mut self, peer: &PeerId, reason: &str) {
        if let Some(conn) = self.connected_peers.remove(peer) {
            conn.conn.close(ERRNO_CLOSE_USR, reason.as_bytes())
        }
    }
}
