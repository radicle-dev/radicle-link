// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{fmt::Debug, future::Future, net::SocketAddr, sync::Arc};

use async_stream::stream;
use futures::{stream::BoxStream, StreamExt};
use link_async::Spawner;
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
    git::storage,
    net::replication::{self, Replication},
    paths::Paths,
    rate_limit::RateLimiter,
    PeerId,
    Signer,
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
mod tick;

mod tincans;
pub(super) use tincans::TinCans;
pub use tincans::{Connected, Interrogation, RecvError};

mod state;
pub use state::Quota;
use state::{RateLimits, State, StateConfig, Storage};

pub type Endpoint = quic::Endpoint<2>;

#[derive(Clone, Debug)]
pub struct Config {
    pub paths: Paths,
    pub listen_addr: SocketAddr,
    pub advertised_addrs: Option<NonEmpty<SocketAddr>>,
    pub membership: membership::Params,
    pub network: Network,
    pub replication: replication::Config,
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
    spawner: Arc<Spawner>,
    phone: TinCans,
    config: Config,
    signer: Sign,
    replication: Replication,
    storage: Store,
    caches: cache::Caches,
) -> Result<Bound<Store>, error::Bootstrap>
where
    Sign: Signer + Clone + Send + Sync + 'static,
    Store: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
{
    let local_id = PeerId::from_signer(&signer);
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
    let gossip = broadcast::State::new(Storage::new(storage, config.rate_limits.storage), ());
    let limits = RateLimits {
        membership: Arc::new(RateLimiter::keyed(
            config.rate_limits.membership,
            nonzero!(1024 * 1024usize),
        )),
    };

    let state = State {
        local_id,
        endpoint,
        membership,
        replication,
        gossip,
        phone: phone.clone(),
        config: StateConfig {
            paths: Arc::new(config.paths),
        },
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
    #[cfg(not(feature = "replication-v3"))]
    let git_factory = {
        use crate::git::p2p::transport::{self, GitStreamFactory};

        let gf = Arc::new(Box::new(state.clone()) as Box<dyn GitStreamFactory>);
        transport::register().register_stream_factory(state.local_id, Arc::downgrade(&gf));
        gf
    };

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
            #[cfg(not(feature = "replication-v3"))]
            drop(git_factory);
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
