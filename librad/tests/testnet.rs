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

use std::{
    future::Future,
    io,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
};

use futures::{
    future,
    stream::{self, StreamExt, TryStreamExt},
};
use lazy_static::lazy_static;
use tempfile::{tempdir, TempDir};

use librad::{
    keys::SecretKey,
    net::{
        peer::{BindError, BoundPeer, Handle, Peer},
        protocol::ProtocolEvent,
    },
    paths::Paths,
};

lazy_static! {
    static ref LOCALHOST_ANY: SocketAddr =
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 0));
}

pub struct TestPeer {
    _tmp: TempDir,
    pub peer: Peer,
}

impl TestPeer {
    async fn bind<'a>(&self) -> Result<BoundPeer<'a>, BindError> {
        self.peer.clone().bind(*LOCALHOST_ANY).await
    }
}

pub fn setup(num_peers: usize) -> anyhow::Result<Vec<TestPeer>> {
    let mut peers = Vec::with_capacity(num_peers);

    for _ in 0..num_peers {
        let tmp = tempdir()?;
        let paths = Paths::from_root(tmp.path())?;
        let key = SecretKey::new();
        let peer = Peer::init(paths, key)?;
        peers.push(TestPeer { _tmp: tmp, peer });
    }

    Ok(peers)
}

pub async fn bind<'a, I>(peers: I) -> Result<Vec<BoundPeer<'a>>, BindError>
where
    I: IntoIterator<Item = &'a TestPeer>,
{
    let mut bound_peers = Vec::new();
    for test_peer in peers {
        bound_peers.push(test_peer.bind().await?);
    }
    Ok(bound_peers)
}

pub async fn run<'a, F>(bound_peers: Vec<BoundPeer<'a>>, shutdown: F) -> Result<(), io::Error>
where
    F: Future<Output = ()> + Send + Clone + Unpin,
{
    assert!(!bound_peers.is_empty());

    let (seed_id, seed_addr) = {
        let seed = bound_peers.first().unwrap();
        (seed.peer_id().clone(), seed.bound_addr()?)
    };

    stream::iter(bound_peers)
        .map(Ok)
        .try_for_each_concurrent(None, |peer| {
            let shutdown = shutdown.clone();
            let addrs = if &seed_id == peer.peer_id() {
                vec![]
            } else {
                vec![(seed_id.clone(), seed_addr)]
            };
            async move {
                peer.run(addrs, shutdown).await;
                Ok(())
            }
        })
        .await
}

pub async fn wait_connected(handles: Vec<Handle>, min_connected: usize) {
    let events = future::join_all(handles.iter().map(|handle| handle.subscribe())).await;
    stream::select_all(events)
        .scan(0, |connected, event| {
            if let ProtocolEvent::Connected(_) = event {
                *connected += 1;
            }

            future::ready(if *connected < min_connected {
                Some(event)
            } else {
                None
            })
        })
        .collect::<Vec<_>>()
        .await;
}
