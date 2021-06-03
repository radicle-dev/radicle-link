// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{fmt::Debug, future::Future, net::SocketAddr, sync::Arc, time::Duration};

use futures::{channel::mpsc, stream::StreamExt as _};
use nonempty::NonEmpty;
use rand_pcg::Pcg64Mcg;
use tracing::Instrument as _;

use super::{
    connection::{LocalAddr, LocalPeer},
    quic,
    upgrade,
    Network,
};
use crate::{
    executor,
    git::{
        self,
        p2p::{server::GitServer, transport::GitStreamFactory},
        replication,
        storage,
    },
    paths::Paths,
    signer::Signer,
    PeerId,
};

pub mod broadcast;
pub mod error;
pub mod event;
pub mod gossip;
pub mod interrogation;
pub mod io;
pub mod membership;

mod info;
pub use info::{Capability, PartialPeerInfo, PeerAdvertisement, PeerInfo};

mod accept;
mod cache;
mod control;
mod nonce;
mod tick;

mod tincans;
pub(super) use tincans::TinCans;
pub use tincans::{Interrogation, RecvError};

mod state;
pub use state::Quota;
use state::{RateLimits, State, StateConfig, Storage};

#[derive(Clone, Debug)]
pub struct Config {
    pub paths: Paths,
    pub listen_addr: SocketAddr,
    pub advertised_addrs: Option<NonEmpty<SocketAddr>>,
    pub membership: membership::Params,
    pub network: Network,
    pub replication: replication::Config,
    pub fetch: config::Fetch,
    pub rate_limits: Quota,
    // TODO: transport, ...
}

pub mod config {
    use std::time::Duration;

    #[derive(Clone, Copy, Debug)]
    pub struct Fetch {
        pub fetch_slot_wait_timeout: Duration,
    }

    impl Default for Fetch {
        fn default() -> Self {
            Self {
                fetch_slot_wait_timeout: Duration::from_secs(20),
            }
        }
    }
}

/// Binding of a peer to a network socket.
///
/// Created by [`crate::net::peer::Peer::bind`]. Call [`Bound::accept`] to start
/// accepting connections from peers.
pub struct Bound<S> {
    phone: TinCans,
    state: State<S>,
    incoming: quic::IncomingConnections<'static>,
    periodic: mpsc::Receiver<membership::Periodic<SocketAddr>>,
}

impl<S> Bound<S> {
    pub fn peer_id(&self) -> PeerId {
        self.state.local_id
    }

    pub fn listen_addrs(&self) -> Vec<SocketAddr> {
        self.state.endpoint.listen_addrs()
    }

    /// Start accepting connections from remote peers.
    ///
    /// Returns a tuple of two futures, where:
    ///
    /// * The first future will gracefully terminate the endpoint when polled
    /// * The second future drives the network stack
    ///
    /// The second future is typically scheduled as a task onto the async
    /// runtime (`spawn`), while the first one should be kept around and
    /// polled when (but not before!) either the accept loop should be
    /// terminated early, or the second future returned an error.
    pub fn accept<D>(
        self,
        disco: D,
    ) -> (
        impl Future<Output = ()>,
        impl Future<Output = Result<!, quic::Error>>,
    )
    where
        S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
        D: futures::Stream<Item = (PeerId, Vec<SocketAddr>)> + Send + 'static,
    {
        accept(self, disco)
    }
}

impl<S> LocalPeer for Bound<S> {
    fn local_peer_id(&self) -> PeerId {
        self.peer_id()
    }
}

impl<S> LocalAddr for Bound<S> {
    type Addr = SocketAddr;

    fn listen_addrs(&self) -> Vec<Self::Addr> {
        self.state.endpoint.listen_addrs()
    }
}

pub async fn bind<Sign, Store>(
    phone: TinCans,
    config: Config,
    signer: Sign,
    storage: Store,
) -> Result<Bound<Store>, error::Bootstrap>
where
    Sign: Signer + Clone + Send + Sync + 'static,
    Store: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
{
    let spawner = Arc::new(executor::Spawner::new("protocol"));
    let local_id = PeerId::from_signer(&signer);
    let git = GitServer::new(&config.paths);
    let quic::BoundEndpoint { endpoint, incoming } = quic::Endpoint::bind(
        signer,
        &spawner,
        config.listen_addr,
        config.advertised_addrs,
        config.network,
    )
    .await?;
    let (membership, periodic) = membership::Hpv::<_, SocketAddr>::new(
        &spawner,
        local_id,
        Pcg64Mcg::new(rand::random()),
        config.membership,
    );
    let storage = Storage::new(storage, config.rate_limits.storage_errors);
    let state = State {
        local_id,
        endpoint,
        git,
        membership,
        storage,
        phone: phone.clone(),
        config: StateConfig {
            replication: config.replication,
            fetch: config.fetch,
        },
        nonces: nonce::NonceBag::new(Duration::from_secs(300)), // TODO: config
        caches: cache::Caches::default(),
        spawner,
        limits: RateLimits {
            gossip: Arc::new(governor::RateLimiter::keyed(config.rate_limits.gossip)),
            membership: Arc::new(governor::RateLimiter::keyed(config.rate_limits.membership)),
        },
    };

    Ok(Bound {
        phone,
        state,
        incoming,
        periodic,
    })
}

#[tracing::instrument(
    skip(phone, state, incoming, periodic, disco),
    fields(peer_id = %state.local_id),
)]
pub fn accept<Store, Disco>(
    Bound {
        phone,
        state,
        incoming,
        periodic,
    }: Bound<Store>,
    disco: Disco,
) -> (
    impl Future<Output = ()>,
    impl Future<Output = Result<!, quic::Error>>,
)
where
    Store: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
    Disco: futures::Stream<Item = (PeerId, Vec<SocketAddr>)> + Send + 'static,
{
    let _git_factory = Arc::new(Box::new(state.clone()) as Box<dyn GitStreamFactory>);
    git::p2p::transport::register()
        .register_stream_factory(state.local_id, Arc::downgrade(&_git_factory));

    let endpoint = state.endpoint.clone();
    let spawner = state.spawner.clone();

    let tasks = [
        spawner.spawn(accept::disco(state.clone(), disco)),
        spawner.spawn(accept::periodic(state.clone(), periodic)),
        spawner.spawn(accept::ground_control(
            state.clone(),
            async_stream::stream! {
                let mut r = phone.downstream.subscribe();
                loop { yield r.recv().await; }
            }
            .boxed(),
        )),
    ];
    let run = async move {
        let res = io::connections::incoming(state, incoming).await;
        drop(_git_factory);
        res
    }
    .in_current_span();
    let sht = async move {
        tracing::info!("shutting down...");
        drop(tasks);
        endpoint.shutdown().await;
        tracing::info!("shutdown complete");
    }
    .in_current_span();

    (sht, run)
}

pub trait ProtocolStorage<A>: broadcast::LocalStorage<A> + storage::Pooled + Send + Sync {}
impl<A, T> ProtocolStorage<A> for T where
    T: broadcast::LocalStorage<A> + storage::Pooled + Send + Sync
{
}

impl<R, A> broadcast::Membership for membership::Hpv<R, A>
where
    R: rand::Rng + Clone,
    A: Clone + Debug + Ord,
{
    fn members(&self, exclude: Option<PeerId>) -> Vec<PeerId> {
        self.broadcast_recipients(exclude)
    }

    fn is_member(&self, peer: &PeerId) -> bool {
        self.is_known(peer)
    }
}
