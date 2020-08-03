// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

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
