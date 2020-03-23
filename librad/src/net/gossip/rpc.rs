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

use serde::{Deserialize, Serialize};

use crate::net::gossip::types::{PeerAdvertisement, PeerInfo};

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Rpc<A> {
    Membership(Membership),
    Gossip(Gossip<A>),
}

impl<A> From<Membership> for Rpc<A> {
    fn from(m: Membership) -> Self {
        Self::Membership(m)
    }
}

impl<A> From<Gossip<A>> for Rpc<A> {
    fn from(g: Gossip<A>) -> Self {
        Self::Gossip(g)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Membership {
    Join(PeerAdvertisement),
    ForwardJoin {
        joined: PeerInfo,
        ttl: usize,
    },
    Neighbour(PeerAdvertisement),
    Shuffle {
        origin: PeerInfo,
        peers: Vec<PeerInfo>,
        ttl: usize,
    },
    ShuffleReply {
        peers: Vec<PeerInfo>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Gossip<A> {
    Have { origin: PeerInfo, val: A },
    Want { origin: PeerInfo, val: A },
}
