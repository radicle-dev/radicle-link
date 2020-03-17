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

use crate::{
    net::gossip::types::{PeerAdvertisement, PeerInfo},
    project::ProjectId,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Rpc {
    Membership(Membership),
    Gossip(Gossip),
}

impl From<Membership> for Rpc {
    fn from(m: Membership) -> Self {
        Self::Membership(m)
    }
}

impl From<Gossip> for Rpc {
    fn from(g: Gossip) -> Self {
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
pub enum Gossip {
    Have { origin: PeerInfo, val: Update },
    Want { origin: PeerInfo, val: Update },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Update {
    Project {
        project: ProjectId,
        head: Option<Ref>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ref {
    name: String,
    target: Vec<u8>,
}
