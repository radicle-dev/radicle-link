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
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    ops::Deref,
};

use futures::{
    future::{self, Either, FutureExt},
    stream::{self, StreamExt},
};
use lazy_static::lazy_static;
use tempfile::{tempdir, TempDir};

use librad::{
    git,
    keys::SecretKey,
    net::{
        discovery,
        peer::{Peer, PeerApi, PeerConfig},
        protocol::ProtocolEvent,
    },
    paths::Paths,
    peer::PeerId,
};

lazy_static! {
    static ref LOCALHOST_ANY: SocketAddr =
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 0));
}

pub struct TestPeer {
    _tmp: TempDir,
    pub peer: Peer,
}

impl Deref for TestPeer {
    type Target = Peer;

    fn deref(&self) -> &Self::Target {
        &self.peer
    }
}

impl AsRef<Peer> for TestPeer {
    fn as_ref(&self) -> &Peer {
        self
    }
}

async fn boot(seeds: Vec<(PeerId, SocketAddr)>) -> anyhow::Result<TestPeer> {
    let tmp = tempdir()?;
    let paths = Paths::from_root(tmp.path())?;
    let key = SecretKey::new();
    let listen_addr = *LOCALHOST_ANY;
    let gossip_params = Default::default();
    let disco = discovery::Static::new(seeds);

    git::storage::Storage::init(&paths, key.clone())?;

    let config = PeerConfig {
        key,
        paths,
        listen_addr,
        gossip_params,
        disco,
    };

    config
        .try_into_peer()
        .await
        .map(|peer| TestPeer { _tmp: tmp, peer })
        .map_err(|e| e.into())
}

pub async fn setup(num_peers: usize) -> anyhow::Result<Vec<TestPeer>> {
    if num_peers < 1 {
        return Ok(vec![]);
    }

    let seed = boot(vec![]).await?;
    let seed_addrs = vec![(seed.peer_id(), seed.listen_addr())];

    let mut peers = Vec::with_capacity(num_peers);
    peers.push(seed);

    for _ in 1..num_peers {
        let peer = boot(seed_addrs.clone()).await?;
        peers.push(peer)
    }

    Ok(peers)
}

pub async fn run_on_testnet<F, Fut, A>(peers: Vec<TestPeer>, mut f: F) -> A
where
    F: FnMut(Vec<PeerApi>) -> Fut,
    Fut: Future<Output = A>,
{
    let len = peers.len();

    // move out tempdirs, so they don't get dropped
    let (_tmps, peers) = peers
        .into_iter()
        .map(|TestPeer { _tmp, peer }| (_tmp, peer))
        .unzip::<_, _, Vec<_>, Vec<_>>();

    let (apis, runners) = peers
        .into_iter()
        .map(|peer| peer.accept().unwrap())
        .unzip::<_, _, Vec<_>, Vec<_>>();

    let events = {
        let mut events = Vec::with_capacity(len);
        for api in &apis {
            events.push(api.protocol().subscribe().await);
        }
        events
    };
    let connected = wait_connected(events, len);

    let res = future::select(
        future::select_all(runners).boxed(),
        Box::pin(async {
            connected.await;
            f(apis).await
        }),
    )
    .await;

    match res {
        Either::Left(_) => unreachable!(),
        Either::Right((output, _)) => output,
    }
}

pub async fn wait_connected<S>(events: Vec<S>, min_connected: usize)
where
    S: futures::Stream<Item = ProtocolEvent> + Unpin,
{
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
