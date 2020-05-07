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

use serde::{Deserialize, Serialize};

use crate::net::gossip::types::{PeerAdvertisement, PeerInfo};

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Rpc<Addr, Payload>
where
    Addr: Clone + PartialEq + Eq + Hash,
{
    Membership(Membership<Addr>),
    Gossip(Gossip<Addr, Payload>),
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Membership<Addr>
where
    Addr: Clone + PartialEq + Eq + Hash,
{
    Join(PeerAdvertisement<Addr>),
    ForwardJoin {
        joined: PeerInfo<Addr>,
        ttl: usize,
    },
    Neighbour(PeerAdvertisement<Addr>),
    Shuffle {
        origin: PeerInfo<Addr>,
        peers: Vec<PeerInfo<Addr>>,
        ttl: usize,
    },
    ShuffleReply {
        peers: Vec<PeerInfo<Addr>>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Gossip<Addr, Payload>
where
    Addr: Clone + PartialEq + Eq + Hash,
{
    Have {
        origin: PeerInfo<Addr>,
        val: Payload,
    },
    Want {
        origin: PeerInfo<Addr>,
        val: Payload,
    },
}
