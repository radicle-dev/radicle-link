// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    future::Future,
    io,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    ops::Deref,
};

use futures::{
    future::{self, FutureExt as _},
    stream::{StreamExt as _, TryStreamExt as _},
};
use tempfile::{tempdir, TempDir};

use librad::{
    git,
    keys::SecretKey,
    net::{
        connection::{LocalAddr, LocalPeer},
        discovery::{self, Discovery as _},
        peer::{self, Peer},
        protocol,
    },
    paths::Paths,
    peer::PeerId,
    std_ext::iter::IteratorExt as _,
};

lazy_static! {
    static ref LOCALHOST_ANY: SocketAddr =
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 0));
}

pub struct BoundTestPeer {
    tmp: TempDir,
    peer: Peer<SecretKey>,
    bound: protocol::Bound<peer::PeerStorage>,
    disco: discovery::Static,
}

impl BoundTestPeer {
    pub fn listen_addrs(&self) -> io::Result<Vec<SocketAddr>> {
        self.bound.listen_addrs()
    }
}

impl LocalPeer for BoundTestPeer {
    fn local_peer_id(&self) -> PeerId {
        self.peer.peer_id()
    }
}

impl LocalAddr for BoundTestPeer {
    type Addr = SocketAddr;

    fn listen_addrs(&self) -> io::Result<Vec<Self::Addr>> {
        self.bound.listen_addrs()
    }
}

pub struct RunningTestPeer {
    _tmp: TempDir,
    peer: Peer<SecretKey>,
    listen_addrs: Vec<SocketAddr>,
}

impl Deref for RunningTestPeer {
    type Target = Peer<SecretKey>;

    fn deref(&self) -> &Self::Target {
        &self.peer
    }
}

impl RunningTestPeer {
    pub fn listen_addrs(&self) -> &[SocketAddr] {
        &self.listen_addrs
    }
}

impl LocalPeer for RunningTestPeer {
    fn local_peer_id(&self) -> PeerId {
        self.peer.peer_id()
    }
}

impl LocalAddr for RunningTestPeer {
    type Addr = SocketAddr;

    fn listen_addrs(&self) -> io::Result<Vec<Self::Addr>> {
        Ok(self.listen_addrs.clone())
    }
}

pub async fn boot<I, J>(seeds: I) -> anyhow::Result<BoundTestPeer>
where
    I: IntoIterator<Item = (PeerId, J)>,
    J: IntoIterator<Item = SocketAddr>,
{
    let tmp = tempdir()?;
    let paths = Paths::from_root(tmp.path())?;
    let key = SecretKey::new();

    git::storage::Storage::init(&paths, key.clone())?;

    let listen_addr = *LOCALHOST_ANY;
    let protocol = protocol::Config {
        paths,
        listen_addr,
        advertised_addrs: None,
        membership: Default::default(),
        network: Default::default(),
        replication: Default::default(),
    };
    let disco = seeds.into_iter().collect::<discovery::Static>();
    let storage_pools = peer::PoolSizes::default();

    let peer = Peer::new(peer::Config {
        signer: key,
        protocol,
        storage_pools,
    });
    let bound = peer.bind().await?;

    Ok(BoundTestPeer {
        tmp,
        peer,
        bound,
        disco,
    })
}

/// Setup a testnet with the given number of peers.
/// Peer X+1 has peer X as a seed peer.
pub async fn setup(num_peers: usize) -> anyhow::Result<Vec<BoundTestPeer>> {
    if num_peers < 1 {
        return Ok(vec![]);
    }

    let mut peers = Vec::with_capacity(num_peers);
    let mut seed_addrs: Option<(PeerId, Vec<SocketAddr>)> = None;
    for _ in 0..num_peers {
        let peer = boot(seed_addrs.take()).await?;
        seed_addrs = Some((peer.bound.peer_id(), peer.bound.listen_addrs().unwrap()));
        peers.push(peer)
    }

    Ok(peers)
}

pub async fn setup_disconnected(num_peers: usize) -> anyhow::Result<Vec<BoundTestPeer>> {
    if num_peers < 1 {
        return Ok(vec![]);
    }

    let mut peers = Vec::with_capacity(num_peers);
    for _ in 0..num_peers {
        let peer = boot::<Option<_>, Option<_>>(None).await?;
        peers.push(peer)
    }

    Ok(peers)
}

pub async fn run_on_testnet<F, Fut, A>(
    peers: Vec<BoundTestPeer>,
    min_connected: usize,
    mut f: F,
) -> A
where
    F: FnMut(Vec<RunningTestPeer>) -> Fut,
    Fut: Future<Output = A>,
{
    let (running, bound_tasks, events) = peers
        .into_iter()
        .map(
            |BoundTestPeer {
                 tmp,
                 peer,
                 bound,
                 disco,
             }| {
                let events = peer.subscribe();
                let running = RunningTestPeer {
                    _tmp: tmp,
                    peer,
                    listen_addrs: bound.listen_addrs().unwrap(),
                };
                let bound_task = tokio::task::spawn(bound.accept(disco.discover()));

                (running, bound_task, events)
            },
        )
        .unzip3::<_, _, _, Vec<_>, Vec<_>, Vec<_>>();

    wait_converged(events, min_connected).await;
    let res = f(running).await;
    bound_tasks.into_iter().for_each(|task| task.abort());

    res
}

pub async fn wait_converged<E>(events: E, min_connected: usize)
where
    E: IntoIterator,
    E::Item: futures::Stream<Item = Result<protocol::event::Upstream, protocol::RecvError>> + Send,
{
    if min_connected < 2 {
        return;
    }

    let mut pending = events
        .into_iter()
        .map(|stream| {
            stream
                .try_skip_while(|evt| {
                    future::ok(!matches!(evt, protocol::event::Upstream::Membership(_)))
                })
                .map_ok(drop)
                .boxed()
                .into_future()
                .map(|(x, _)| x)
        })
        .collect();
    let mut connected = 0;
    loop {
        let (out, _, rest): (Option<Result<_, _>>, _, _) = future::select_all(pending).await;
        if let Some(()) = out.transpose().unwrap() {
            connected += 1;
            if connected >= min_connected {
                break;
            }
        }
        pending = rest
    }
}
