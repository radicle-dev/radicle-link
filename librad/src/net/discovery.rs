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

//! Discovery of peers during bootstrap, or out-of-band

use std::{
    marker::PhantomData,
    net::{SocketAddr, ToSocketAddrs},
};

use crate::peer::PeerId;

pub struct Static<I, S> {
    iter: I,
    _marker: PhantomData<S>,
}

impl<I, S> Static<I, S>
where
    I: Iterator<Item = (PeerId, S)>,
    S: ToSocketAddrs,
{
    pub fn new<P>(peers: P) -> Self
    where
        P: IntoIterator<IntoIter = I, Item = (PeerId, S)>,
    {
        Self {
            iter: peers.into_iter(),
            _marker: PhantomData,
        }
    }

    pub fn into_stream(self) -> futures::stream::Iter<Self> {
        futures::stream::iter(self)
    }
}

impl<I, S> Iterator for Static<I, S>
where
    I: Iterator<Item = (PeerId, S)>,
    S: ToSocketAddrs,
{
    type Item = (PeerId, Vec<SocketAddr>);

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.find_map(|(peer_id, addrs)| {
            // TODO: we might want to log resolver errors somewhere
            addrs
                .to_socket_addrs()
                .ok()
                .map(|resolved| (peer_id, resolved.collect()))
        })
    }
}
