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

use std::collections::BTreeMap;

use crate::{
    net::{connection::RemoteInfo as _, quic},
    peer::PeerId,
};

#[derive(Clone, Default)]
pub struct Connections(BTreeMap<PeerId, quic::Connection>);

impl Connections {
    pub fn get(&self, peer: &PeerId) -> Option<&quic::Connection> {
        self.0.get(peer)
    }

    pub fn has_connection(&self, to: &PeerId) -> bool {
        self.0.get(to).and(Some(true)).unwrap_or(false)
    }

    pub fn insert(&mut self, conn: quic::Connection) -> Option<quic::Connection> {
        let peer = conn.remote_peer_id();
        self.0.insert(peer, conn)
    }

    pub fn remove(&mut self, conn: &quic::Connection) -> bool {
        let peer = conn.remote_peer_id();
        let connid = conn.stable_id();
        match self.0.get(&peer) {
            None => false,
            Some(found) => {
                if found.stable_id() == connid {
                    self.0.remove(&peer);
                    true
                } else {
                    false
                }
            },
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&PeerId, &quic::Connection)> {
        self.0.iter()
    }
}
