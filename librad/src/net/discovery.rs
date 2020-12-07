// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! Discovery of peers during bootstrap, or out-of-band

use std::{
    marker::PhantomData,
    net::{SocketAddr, ToSocketAddrs},
};

use crate::peer::PeerId;

pub trait Discovery {
    type Addr;
    type Stream: futures::Stream<Item = (PeerId, Vec<Self::Addr>)> + Send;

    fn discover(self) -> Self::Stream;
}

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

impl<I> Discovery for Static<I, SocketAddr>
where
    I: Iterator<Item = (PeerId, SocketAddr)> + Send,
{
    type Addr = SocketAddr;
    type Stream = futures::stream::Iter<Self>;

    fn discover(self) -> Self::Stream {
        self.into_stream()
    }
}
