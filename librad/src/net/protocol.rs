// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{fmt::Debug, future::Future, net::SocketAddr, sync::Arc, time::Duration};

use async_stream::stream;
use futures::{stream::BoxStream, StreamExt};
use nonempty::NonEmpty;
use nonzero_ext::nonzero;
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
    rate_limit::RateLimiter,
    signer::Signer,
    PeerId,
};

pub mod broadcast;

pub mod cache;
pub use cache::Caches;

pub mod error;
pub mod event;
pub mod gossip;
pub mod interrogation;
pub mod io;
pub mod membership;

mod info;
pub use info::{Capability, PartialPeerInfo, PeerAdvertisement, PeerInfo};

mod accept;

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
    periodic: BoxStream<'static, membership::Periodic<SocketAddr>>,
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
    /// Returns a tuple of
    ///
    /// * a function which, when called, will interrupt the accept loop
    /// * and a future which must be polled to advance the networking stack
    ///
    /// The future runs indefinitely until a fatal error occurs, such as the
    /// endpoint shutting down. It is important to ensure that the future is
    /// **driven to completion** in order to ensure a graceful shutdown.
    pub fn accept<D>(
        self,
        disco: D,
    ) -> (
        impl FnOnce(),
        impl Future<Output = Result<!, io::error::Accept>>,
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
    spawner: Arc<executor::Spawner>,
    phone: TinCans,
    config: Config,
    signer: Sign,
    storage: Store,
    caches: cache::Caches,
) -> Result<Bound<Store>, error::Bootstrap>
where
    Sign: Signer + Clone + Send + Sync + 'static,
    Store: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
{
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
        local_id,
        Pcg64Mcg::new(rand::random()),
        config.membership,
    );
    let storage = Storage::new(storage, config.rate_limits.storage);
    // TODO: make configurable
    let nonces = nonce::NonceBag::new(Duration::from_secs(300));
    let limits = RateLimits {
        membership: Arc::new(RateLimiter::keyed(
            config.rate_limits.membership,
            nonzero!(1024 * 1024usize),
        )),
    };

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
        nonces,
        caches,
        spawner,
        limits,
    };

    Ok(Bound {
        phone,
        state,
        incoming,
        periodic: periodic.boxed(),
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
    impl FnOnce(),
    impl Future<Output = Result<!, io::error::Accept>>,
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
            stream! {
                let mut r = phone.downstream.subscribe();
                loop { yield r.recv().await; }
            },
        )),
    ];
    let run = {
        let endpoint = endpoint.clone();
        async move {
            let res = io::connections::incoming(state, incoming).await;
            drop(_git_factory);
            tracing::debug!("waiting on idle connections...");
            endpoint.wait_idle().await;
            drop(tasks);
            tracing::debug!("protocol shut down");
            res
        }
        .in_current_span()
    };

    (move || endpoint.close(), run)
}

pub trait ProtocolStorage<A>:
    broadcast::LocalStorage<A> + storage::Pooled<storage::Storage> + Send + Sync
{
}
impl<A, T> ProtocolStorage<A> for T where
    T: broadcast::LocalStorage<A> + storage::Pooled<storage::Storage> + Send + Sync
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
