// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use crate::net::protocol::info::{PeerAdvertisement, PeerInfo};

#[derive(Debug, Clone, PartialEq, minicbor::Encode, minicbor::Decode)]
pub enum Message<Addr>
where
    Addr: Clone + Ord,
{
    #[n(0)]
    #[cbor(array)]
    Join {
        #[n(0)]
        info: PeerAdvertisement<Addr>,
    },

    #[n(1)]
    #[cbor(array)]
    ForwardJoin {
        #[n(0)]
        joined: PeerInfo<Addr>,
        #[n(1)]
        ttl: usize,
    },

    #[n(2)]
    #[cbor(array)]
    Neighbour {
        #[n(0)]
        info: PeerAdvertisement<Addr>,
        #[n(1)]
        need_friends: Option<()>,
    },

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

    #[n(5)]
    #[cbor(array)]
    Disconnect,
}
