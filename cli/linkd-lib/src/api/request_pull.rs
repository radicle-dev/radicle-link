use std::net::SocketAddr;

use librad::{git::Urn, net::protocol::request_pull, PeerId};

#[derive(Clone, Debug, PartialEq, Eq, minicbor::Decode, minicbor::Encode)]
pub struct Request {
    #[n(0)]
    pub urn: Urn,
    #[n(1)]
    pub peer: PeerId,
    #[n(2)]
    pub addrs: Vec<SocketAddr>,
}

#[derive(Clone, Debug, PartialEq, Eq, minicbor::Decode, minicbor::Encode)]
#[cbor(transparent)]
pub struct Response(#[n(0)] request_pull::Success);

impl From<request_pull::Success> for Response {
    fn from(x: request_pull::Success) -> Self {
        Self(x)
    }
}
