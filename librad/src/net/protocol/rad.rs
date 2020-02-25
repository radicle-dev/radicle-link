use std::{
    collections::{HashMap, HashSet},
    io,
    net::SocketAddr,
};

use futures::{sink::SinkExt, stream::TryStreamExt, AsyncRead, AsyncWrite};
use futures_codec::{CborCodec, CborCodecError, Framed};
use serde::{Deserialize, Serialize};

use crate::{
    net::connection::{CloseReason, Connection, Stream},
    paths::Paths,
    peer::PeerId,
    project::{Project, ProjectId},
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Membership {
    Join(PeerInfo),
    ForwardJoin {
        joined: (PeerId, PeerAddrInfo),
        hops: u8,
    },
    Neighbour(PeerInfo),
    Shuffle {
        origin: (PeerId, PeerInfo),
        peers: Vec<(PeerId, PeerAddrInfo)>,
        hops: u8,
    },
    ShuffleReply {
        peers: Vec<(PeerId, PeerAddrInfo)>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Gossip {
    Have(ProjectId),
    Want(ProjectId),
    Prune,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Rpc {
    Membership(Membership),
    Gossip(Gossip),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Request {
    GetPeerInfo,
    GetProjects,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Response {
    PeerInfo(PeerInfo),
    // TODO(kim): Due to its object model, `serde` has no obvious way to support
    // indefinite-length arrays (as defined in CBOR). We need to either trick it
    // into it (like, some kind of `Deserialize` for an iterator type), or
    // implement pagination for this.
    Projects(Vec<ProjectId>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerInfo {
    listen_port: u16,
    capabilities: HashSet<Capability>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum Capability {
    Reserved = 0,
}

#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display = "Invalid payload")]
    InvalidPayload(#[fail(cause)] serde_cbor::Error),

    #[fail(display = "No answer to request {:?} from {}", req, from)]
    NoAnswerTo { req: Request, from: PeerId },

    #[fail(display = "Connection to self")]
    SelfConnection,

    #[fail(display = "{}", 0)]
    Io(#[fail(cause)] io::Error),
}

impl From<CborCodecError> for Error {
    fn from(err: CborCodecError) -> Self {
        match err {
            CborCodecError::Cbor(e) => Self::InvalidPayload(e),
            CborCodecError::Io(e) => Self::Io(e),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerAddrInfo {
    capabilities: HashSet<Capability>,
    // TODO(kim): make this a priority set
    addrs: HashSet<SocketAddr>,
}

impl PeerAddrInfo {
    pub fn new(capabilities: HashSet<Capability>) -> Self {
        Self {
            capabilities,
            addrs: HashSet::new(),
        }
    }
}

pub type NegotiatedStream<T> = Framed<T, CborCodec<Rpc, Rpc>>;

#[derive(Debug, Clone)]
struct Config {
    prwl: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self { prwl: 3 }
    }
}

#[derive(Clone)]
pub struct Protocol {
    local_id: PeerId,
    local_info: PeerInfo,
    paths: Paths,
    config: Config,

    connected_peers: HashMap<PeerId, Connection>,
    known_peers: HashMap<PeerId, PeerAddrInfo>,
    providers: HashMap<ProjectId, HashSet<PeerId>>,
}

impl Protocol {
    pub fn new(local_id: &PeerId, local_info: PeerInfo, paths: &Paths) -> Self {
        Self {
            local_id: local_id.clone(),
            local_info,
            paths: paths.clone(),
            config: Default::default(),

            connected_peers: Default::default(),
            known_peers: Default::default(),
            providers: Default::default(),
        }
    }

    pub async fn on_outgoing<T>(
        &mut self,
        conn: Connection,
        mut stream: NegotiatedStream<T>,
        joining: bool,
    ) -> Result<(), Error>
    where
        T: AsyncRead + AsyncWrite + Unpin,
    {
        let remote_id = conn.peer_id();
        // This should not be possible, as we prevent it in the TLS handshake.
        // Leaving it here regardless as a sanity check.
        if remote_id == &self.local_id {
            return Err(Error::SelfConnection);
        }

        {
            let hello = Rpc::Membership(if joining {
                Membership::Join(self.local_info.clone())
            } else {
                Membership::Neighbour(self.local_info.clone())
            });

            stream.send(hello).await?;
        }

        if let Some(old) = self.connected_peers.insert(remote_id.clone(), conn.clone()) {
            old.close(CloseReason::DuplicateConnection)
        }

        // TODO(kim): initiate periodic shuffle
        let res = self.handle_incoming(&conn, stream).await;

        if let Some(conn) = self.connected_peers.remove(remote_id) {
            if res.is_ok() {
                conn.close(CloseReason::ProtocolDisconnect)
            }
        }

        res
    }

    async fn handle_incoming<T>(
        &mut self,
        conn: &Connection,
        mut stream: NegotiatedStream<T>,
    ) -> Result<(), Error>
    where
        T: AsyncRead + AsyncWrite + Unpin,
    {
        while let Some(rpc) = stream.try_next().await? {
            match rpc {
                Rpc::Membership(msg) => match msg {
                    Membership::Join(info) => {
                        let addr_info = {
                            let mut addr = conn.remote_address();
                            addr.set_port(info.listen_port);
                            PeerAddrInfo {
                                capabilities: info.capabilities,
                                addrs: vec![addr].into_iter().collect(),
                            }
                        };

                        self.add_to_known((conn.peer_id().clone(), addr_info.clone()));
                        self.broadcast(Membership::ForwardJoin {
                            joined: (conn.peer_id().clone(), addr_info),
                            hops: self.config.prwl,
                        })
                        .await
                    },

                    Membership::ForwardJoin { joined, hops } => {
                        if hops == 0 {
                            self.try_connect(joined).await
                        } else {
                            self.broadcast(Membership::ForwardJoin {
                                joined,
                                hops: hops - 1,
                            })
                            .await
                        }
                    },

                    Membership::Neighbour(info) => {
                        let mut addr = conn.remote_address();
                        addr.set_port(info.listen_port);
                        self.add_to_known((
                            conn.peer_id().clone(),
                            PeerAddrInfo {
                                capabilities: info.capabilities.clone(),
                                addrs: vec![addr].into_iter().collect(),
                            },
                        ));
                    },

                    Membership::Shuffle {
                        origin: (origin_peer, origin_info),
                        peers,
                        hops,
                    } => {
                        // We're supposed to only remember shuffled peers at
                        // the end of the random walk. Do it anyway for now.
                        peers.into_iter().for_each(|peer| self.add_to_known(peer));

                        if hops > 0 {
                            self.try_send_to(origin_peer, origin_info.listen_port).await
                        }
                    },

                    Membership::ShuffleReply { peers } => {
                        peers.into_iter().for_each(|peer| self.add_to_known(peer))
                    },
                },

                Rpc::Gossip(msg) => {},
            }
        }

        Ok(())
    }

    fn add_to_known(&mut self, (peer, addr_info): (PeerId, PeerAddrInfo)) {
        let entry = self
            .known_peers
            .entry(peer)
            .or_insert_with(|| addr_info.clone());
        entry.addrs = entry.addrs.union(&addr_info.addrs).cloned().collect();
    }

    async fn broadcast(&mut self, msg: Membership) {
        unimplemented!()
    }

    async fn try_connect(&self, to: (PeerId, PeerAddrInfo)) {
        unimplemented!()
    }

    async fn try_send_to(&self, peer: PeerId, port: u16) {
        unimplemented!()
    }

    pub async fn outgoing(&mut self, conn: Connection, stream: Stream) -> Result<(), Error> {
        if conn.peer_id() == &self.local_id {
            return Err(Error::SelfConnection);
        }

        let mut stream = Framed::new(stream, CborCodec::<Request, Response>::new());

        stream.send(Request::GetPeerInfo).await?;
        match stream.try_next().await? {
            Some(Response::PeerInfo(info)) => {
                {
                    let mut addr = conn.remote_address();
                    addr.set_port(info.listen_port);

                    self.known_peers
                        .entry(conn.peer_id().clone())
                        .or_insert_with(|| PeerAddrInfo::new(info.capabilities))
                        .addrs
                        .insert(addr);
                }

                if let Some(old) = self.connected_peers.insert(conn.peer_id().clone(), conn) {
                    old.close(CloseReason::DuplicateConnection)
                }

                Ok(())
            },

            // hrm
            Some(_) => Ok(()),

            None => Err(Error::NoAnswerTo {
                req: Request::GetPeerInfo,
                from: conn.peer_id().clone(),
            }),
        }
    }

    pub async fn incoming<S>(&self, stream: S) -> Result<(), Error>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let mut stream = Framed::new(stream, CborCodec::<Response, Request>::new());
        while let Some(req) = stream.try_next().await? {
            let resp = match req {
                Request::GetPeerInfo => Response::PeerInfo(self.local_info.clone()),
                Request::GetProjects => Response::Projects(Project::list(&self.paths).collect()),
            };

            stream.send(resp).await?
        }

        Ok(())
    }
}
