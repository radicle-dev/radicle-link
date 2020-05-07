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

use std::{collections::HashSet, hash::Hash};

use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

use crate::peer::PeerId;

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum Capability {
    Reserved = 0,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerInfo<Addr>
where
    Addr: Clone + PartialEq + Eq + Hash,
{
    pub peer_id: PeerId,
    pub advertised_info: PeerAdvertisement<Addr>,
    pub seen_addrs: HashSet<Addr>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerAdvertisement<Addr>
where
    Addr: Clone + PartialEq + Eq + Hash,
{
    pub listen_addr: Addr,
    pub capabilities: HashSet<Capability>,
}

impl<Addr> PeerAdvertisement<Addr>
where
    Addr: Clone + PartialEq + Eq + Hash,
{
    pub fn new(listen_addr: Addr) -> Self {
        Self {
            listen_addr,
            capabilities: HashSet::default(),
        }
    }
}
