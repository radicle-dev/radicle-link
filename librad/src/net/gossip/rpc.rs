// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::hash::Hash;

use minicbor::{Decode, Encode};

use crate::net::gossip::types::{PeerAdvertisement, PeerInfo};

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub enum Rpc<Addr, Payload>
where
    Addr: Clone + PartialEq + Eq + Hash,
{
    #[n(0)]
    Membership(#[n(0)] Membership<Addr>),

    #[n(1)]
    Gossip(#[n(0)] Gossip<Addr, Payload>),
}

impl<A, P> From<Membership<A>> for Rpc<A, P>
where
    A: Clone + PartialEq + Eq + Hash,
{
    fn from(m: Membership<A>) -> Self {
        Self::Membership(m)
    }
}

impl<A, P> From<Gossip<A, P>> for Rpc<A, P>
where
    A: Clone + PartialEq + Eq + Hash,
{
    fn from(g: Gossip<A, P>) -> Self {
        Self::Gossip(g)
    }
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub enum Membership<Addr>
where
    Addr: Clone + PartialEq + Eq + Hash,
{
    #[n(0)]
    Join(#[n(0)] PeerAdvertisement<Addr>),

    #[n(1)]
    #[cbor(array)]
    ForwardJoin {
        #[n(0)]
        joined: PeerInfo<Addr>,
        #[n(1)]
        ttl: usize,
    },

    #[n(2)]
    Neighbour(#[n(0)] PeerAdvertisement<Addr>),

    #[n(3)]
    #[cbor(array)]
    Shuffle {
        #[n(0)]
        origin: PeerInfo<Addr>,
        #[n(1)]
        peers: Vec<PeerInfo<Addr>>,
        #[n(2)]
        ttl: usize,
    },

    #[n(4)]
    #[cbor(array)]
    ShuffleReply {
        #[n(0)]
        peers: Vec<PeerInfo<Addr>>,
    },
}

/// Gossip messages are either announcements (`Have`), or queries (`Want`). The
/// `origin` is the sender of the message -- this field is not modified if a
/// message is relayed.
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub enum Gossip<Addr, Payload>
where
    Addr: Clone + PartialEq + Eq + Hash,
{
    #[n(0)]
    #[cbor(array)]
    Have {
        #[n(0)]
        origin: PeerInfo<Addr>,
        #[n(1)]
        val: Payload,
    },

    #[n(1)]
    #[cbor(array)]
    Want {
        #[n(0)]
        origin: PeerInfo<Addr>,
        #[n(1)]
        val: Payload,
    },
}
