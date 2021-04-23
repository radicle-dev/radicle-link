// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::BTreeMap,
    env,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, ToSocketAddrs as _},
    num::NonZeroUsize,
    ops::Deref,
};

use futures::{
    future::{self, FutureExt as _},
    stream::{StreamExt as _, TryStreamExt as _},
};
use tempfile::{tempdir, TempDir};
use tokio::task::{spawn, JoinHandle};

use librad::{
    git,
    keys::SecretKey,
    net::{
        connection::{LocalAddr, LocalPeer},
        discovery::{self, Discovery as _},
        peer::{self, Peer},
        protocol,
        quic,
        Network,
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
    peer: Peer<SecretKey>,
    bound: protocol::Bound<peer::PeerStorage>,
    disco: discovery::Static,
    tmp: TempDir,
}

impl BoundTestPeer {
    pub fn listen_addrs(&self) -> Vec<SocketAddr> {
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

    fn listen_addrs(&self) -> Vec<Self::Addr> {
        self.bound.listen_addrs()
    }
}

pub struct RunningTestPeer {
    peer: Peer<SecretKey>,
    listen_addrs: Vec<SocketAddr>,
    _tmp: TempDir,
}

// No, this is not sound, but conveniently allows to write tests as if this was
// just a plain `Peer`
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

    fn listen_addrs(&self) -> Vec<Self::Addr> {
        self.listen_addrs.clone()
    }
}

async fn boot<I, J>(seeds: I) -> anyhow::Result<BoundTestPeer>
where
    I: IntoIterator<Item = (PeerId, J)>,
    J: IntoIterator<Item = SocketAddr>,
{
    let tmp = tempdir()?;
    let paths = Paths::from_root(tmp.path())?;
    let key = SecretKey::new();

    // eagerly init so we error out early when it fails
    git::storage::Storage::init(&paths, key.clone())?;

    let listen_addr = *LOCALHOST_ANY;
    let protocol = protocol::Config {
        paths,
        listen_addr,
        advertised_addrs: None,
        membership: Default::default(),
        network: Network::Custom(b"localtestnet".as_ref().into()),
        replication: Default::default(),
        fetch: Default::default(),
    };
    let disco = seeds.into_iter().collect::<discovery::Static>();
    let peer = Peer::new(peer::Config {
        signer: key,
        protocol,
        storage: Default::default(),
    });
    let bound = peer.bind().await?;

    Ok(BoundTestPeer {
        peer,
        bound,
        disco,
        tmp,
    })
}

/// How to bootstrap the test network.
pub enum Bootstrap {
    /// Start up disconnected.
    None,
    /// All peers bootstrap through the first one.
    ///
    /// The first peer start without any boostrap nodes.
    First,
    /// All peers bootstrap through the previously started peer.
    ///
    /// The first peer starts without any bootstrap nodes.
    Prev,
    /// Bootstrap through a fixed set of peers known in advance.
    ///
    /// Useful for running against an already-running network (eg. compose)
    Fixed(Vec<(PeerId, Vec<SocketAddr>)>),
}

impl Bootstrap {
    /// Figure out the [`Bootstrap`] mode from the environment variable
    /// `LIBRAD_TEST_BOOTSTRAP`.
    ///
    /// The values "first" and "prev" map to the respective variants, else the
    /// value is attempted to be parsed as a comma-separated list of
    /// `peer-id@socketaddr` pairs. If the list is empty, [`Bootstrap::
    /// None`] is returned.
    ///
    ///
    /// If the variable is not set, the [`Default`] value is returned (which is
    /// [`Bootstrap::Prev`]).
    ///
    /// # Panics
    ///
    /// This method panics if `LIBRAD_TEST_BOOTSTRAP` contains a string of the
    /// form `peer-id@socketaddr`, but that doesn parse/resolve successfully
    /// into `(PeerId, SocketAddr)`.
    pub fn from_env() -> Self {
        env::var("LIBRAD_TEST_BOOTSTRAP")
            .ok()
            .map(|val| match val.as_str() {
                "first" => Self::First,
                "prev" => Self::Prev,
                x => {
                    let peers = x
                        .split(',')
                        .filter_map(|entry| {
                            entry.split_once('@').map(|(peer_id, addr)| {
                                let peer_id = peer_id.parse::<PeerId>().expect("invalid peer id");
                                let addr = addr
                                    .to_socket_addrs()
                                    .map(|mut a| a.next())
                                    .expect("invalid peer address")
                                    .expect("unable to resolve peer address");
                                (peer_id, addr)
                            })
                        })
                        .fold(BTreeMap::new(), |mut acc, (peer_id, addr)| {
                            acc.entry(peer_id).or_insert_with(Vec::new).push(addr);
                            acc
                        });
                    if peers.is_empty() {
                        Self::None
                    } else {
                        Self::Fixed(peers.into_iter().collect())
                    }
                },
            })
            .unwrap_or_default()
    }
}

impl Default for Bootstrap {
    fn default() -> Self {
        Self::Prev
    }
}

pub struct Config {
    pub num_peers: NonZeroUsize,
    pub min_connected: usize,
    pub bootstrap: Bootstrap,
}

async fn bootstrap(config: Config) -> anyhow::Result<Vec<BoundTestPeer>> {
    let num_peers = config.num_peers.get();
    let mut peers = Vec::with_capacity(num_peers);

    match config.bootstrap {
        Bootstrap::None => {
            for _ in 0..num_peers {
                let peer = boot::<Option<_>, Option<_>>(None).await?;
                peers.push(peer);
            }
        },

        Bootstrap::First => {
            let bootstrap_node = boot::<Option<_>, Option<_>>(None).await?;
            let bootstrap = Some((
                bootstrap_node.bound.peer_id(),
                bootstrap_node.listen_addrs(),
            ));
            peers.push(bootstrap_node);

            for _ in 1..num_peers {
                let peer = boot(bootstrap.clone()).await?;
                peers.push(peer);
            }
        },

        Bootstrap::Prev => {
            let mut bootstrap: Option<(PeerId, Vec<SocketAddr>)> = None;
            for _ in 0..num_peers {
                let peer = boot(bootstrap.take()).await?;
                bootstrap = Some((peer.bound.peer_id(), peer.bound.listen_addrs()));
                peers.push(peer);
            }
        },

        Bootstrap::Fixed(bootstrap) => {
            for _ in 0..num_peers {
                let peer = boot(bootstrap.clone()).await?;
                peers.push(peer);
            }
        },
    }

    Ok(peers)
}

pub struct Testnet {
    peers: Vec<RunningTestPeer>,
    tasks: Vec<JoinHandle<Result<!, quic::Error>>>,
}

impl Testnet {
    pub fn peers(&self) -> &[RunningTestPeer] {
        self.as_ref()
    }
}

impl AsRef<[RunningTestPeer]> for Testnet {
    fn as_ref(&self) -> &[RunningTestPeer] {
        &self.peers
    }
}

impl Drop for Testnet {
    fn drop(&mut self) {
        self.peers.drain(..).for_each(drop);
        for task in &self.tasks {
            task.abort()
        }
    }
}

pub async fn run(config: Config) -> anyhow::Result<Testnet> {
    let min_connected = config.min_connected;
    let peers = bootstrap(config).await?;
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
                    listen_addrs: bound.listen_addrs(),
                };
                let bound_task = spawn(bound.accept(disco.discover()));

                (running, bound_task, events)
            },
        )
        .unzip3::<_, _, _, Vec<_>, Vec<_>, Vec<_>>();

    wait_converged(events, min_connected).await;

    Ok(Testnet {
        peers: running,
        tasks: bound_tasks,
    })
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
