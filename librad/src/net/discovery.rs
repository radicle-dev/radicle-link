// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! Discovery of peers during bootstrap, or out-of-band

use std::{
    collections::{btree_map, BTreeMap},
    io,
    iter::FromIterator,
    net::{SocketAddr, ToSocketAddrs},
};

use crate::peer::PeerId;

pub trait Discovery {
    type Addr;
    type Stream: futures::Stream<Item = (PeerId, Vec<Self::Addr>)> + Send;

    fn discover(self) -> Self::Stream;
}

#[derive(Clone, Default)]
pub struct Static {
    peers: BTreeMap<PeerId, Vec<SocketAddr>>,
}

impl Static {
    pub fn resolve<I, J>(peers: I) -> Result<Self, io::Error>
    where
        I: IntoIterator<Item = (PeerId, J)>,
        J: ToSocketAddrs,
    {
        use btree_map::Entry::*;

        let peers = peers
            .into_iter()
            .map(|(peer, to_addrs)| {
                let addrs = to_addrs.to_socket_addrs()?;
                Ok((peer, addrs.collect::<Vec<_>>()))
            })
            .fold(
                Ok(BTreeMap::new()),
                |acc: Result<BTreeMap<_, _>, io::Error>, res: Result<_, io::Error>| {
                    let (peer, addrs) = res?;
                    let mut acc = acc?;
                    match acc.entry(peer) {
                        Vacant(entry) => {
                            entry.insert(addrs);
                        },
                        Occupied(mut entry) => {
                            entry.get_mut().extend(addrs);
                        },
                    }
                    Ok(acc)
                },
            )?;

        Ok(Self::from(peers))
    }
}

impl From<BTreeMap<PeerId, Vec<SocketAddr>>> for Static {
    fn from(peers: BTreeMap<PeerId, Vec<SocketAddr>>) -> Self {
        Self { peers }
    }
}

// Better inference than `FromIterator`
impl From<Vec<(PeerId, Vec<SocketAddr>)>> for Static {
    fn from(v: Vec<(PeerId, Vec<SocketAddr>)>) -> Self {
        v.into_iter().collect()
    }
}

impl<I> FromIterator<(PeerId, I)> for Static
where
    I: IntoIterator<Item = SocketAddr>,
{
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = (PeerId, I)>,
    {
        use btree_map::Entry::*;

        iter.into_iter()
            .map(|(peer, addrs)| (peer, addrs.into_iter().collect()))
            .fold(
                BTreeMap::<PeerId, Vec<SocketAddr>>::new(),
                |mut acc, (peer, addrs)| {
                    match acc.entry(peer) {
                        Vacant(entry) => {
                            entry.insert(addrs);
                        },
                        Occupied(mut entry) => {
                            entry.get_mut().extend(addrs);
                        },
                    }
                    acc
                },
            )
            .into()
    }
}

impl Discovery for Static {
    type Addr = SocketAddr;
    type Stream = futures::stream::Iter<btree_map::IntoIter<PeerId, Vec<SocketAddr>>>;

    fn discover(self) -> Self::Stream {
        futures::stream::iter(self.peers.into_iter())
    }
}
