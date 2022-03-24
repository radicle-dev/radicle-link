use std::net::SocketAddr;

use librad::{git::Urn, PeerId};

#[derive(Clone, Debug, PartialEq, Eq, minicbor::Decode, minicbor::Encode)]
pub struct RequestPull {
    #[n(0)]
    pub urn: Urn,
    #[n(1)]
    pub peer: PeerId,
    #[n(2)]
    pub addrs: Vec<SocketAddr>,
}
