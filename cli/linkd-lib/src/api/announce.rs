use librad::{git::Urn, net::protocol::gossip, PeerId};
use radicle_git_ext::Oid;

#[derive(Clone, Debug, PartialEq, Eq, minicbor::Decode, minicbor::Encode)]
pub struct Request {
    #[n(0)]
    pub urn: Urn,
    #[n(1)]
    pub rev: Oid,
}

impl Request {
    pub fn into_gossip(self, peer: PeerId) -> gossip::Payload {
        gossip::Payload {
            urn: self.urn,
            rev: Some(self.rev.into()),
            origin: Some(peer),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, minicbor::Decode, minicbor::Encode)]
pub struct Response;
