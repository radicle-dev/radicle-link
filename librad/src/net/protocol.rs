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
use std_ext::Void;
use tracing::Instrument as _;

pub use super::quic::SendOnly;
use super::{
    connection::{LocalAddr, LocalPeer},
    quic,
    upgrade,
    Network,
};
use crate::{
    git::storage,
    net::replication,
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
pub mod request_pull;
pub mod rpc;

mod info;
pub use info::{Capability, PartialPeerInfo, PeerAdvertisement, PeerInfo};

mod accept;

mod control;
mod tick;

mod tincans;
pub(super) use tincans::TinCans;
pub use tincans::{Connected, Interrogation, RecvError, RequestPull};

mod state;
pub use state::Quota;
use state::{RateLimits, State, StateConfig, Storage};

pub type Endpoint = quic::Endpoint<2>;

#[derive(Clone, Debug)]
pub struct Config<Guard = config::DenyAll> {
    pub paths: Paths,
    pub listen_addr: SocketAddr,
    pub advertised_addrs: Option<NonEmpty<SocketAddr>>,
    pub membership: membership::Params,
    pub network: Network,
    pub replication: replication::Config,
    pub rate_limits: Quota,
    pub request_pull: Guard,
    // TODO: transport, ...
}

pub mod config {
    use std::time::Duration;

    use crate::{git::Urn, net::protocol::request_pull::Guard, PeerId};

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

    /// A request-pull [`Guard`] that will always return the [`Denied`] error.
    #[derive(Clone, Copy, Debug)]
    pub struct DenyAll;

    #[derive(Debug, thiserror::Error)]
    #[error("request-pull denied for `{0}`")]
    pub struct Denied(Urn);

    impl Guard for DenyAll {
        type Error = Denied;

        type Output = std::convert::Infallible;

        fn guard(&self, _: &PeerId, urn: &Urn) -> Result<Self::Output, Self::Error> {
            Err(Denied(urn.clone()))
        }
    }
}

/// Binding of a peer to a network socket.
///
/// Created by [`crate::net::peer::Peer::bind`]. Call [`Bound::accept`] to start
/// accepting connections from peers.
pub struct Bound<S, G = config::DenyAll> {
    phone: TinCans,
    state: State<S, G>,
    incoming: quic::IncomingConnections<'static>,
    periodic: BoxStream<'static, membership::Periodic<SocketAddr>>,
}

impl<S, G> Bound<S, G> {
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
        impl Future<Output = Result<Void, io::error::Accept>>,
    )
    where
        S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
        G: RequestPullGuard,
        D: futures::Stream<Item = (PeerId, Vec<SocketAddr>)> + Send + 'static,
    {
        accept(self, disco)
    }
}

impl<S, A> LocalPeer for Bound<S, A> {
    fn local_peer_id(&self) -> PeerId {
        self.peer_id()
    }
}

impl<S, A> LocalAddr for Bound<S, A> {
    type Addr = SocketAddr;

    fn listen_addrs(&self) -> Vec<Self::Addr> {
        self.state.endpoint.listen_addrs()
    }
}

pub async fn bind<Sign, Store, Guard>(
    spawner: Arc<Spawner>,
    phone: TinCans,
    config: Config<Guard>,
    signer: Sign,
    storage: Store,
    caches: cache::Caches,
) -> Result<Bound<Store, Guard>, error::Bootstrap>
where
    Sign: Signer + Clone + Send + Sync + 'static,
    Store: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
    Guard: RequestPullGuard,
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
    let gossip = broadcast::State::new(
        Storage::new(storage.clone(), config.rate_limits.storage.clone()),
        (),
    );
    let request_pull = request_pull::State::new(
        Storage::new(storage, config.rate_limits.storage),
        config.paths.clone(),
        config.request_pull,
    );
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
        gossip,
        request_pull,
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
pub fn accept<Store, Guard, Disco>(
    Bound {
        phone,
        state,
        incoming,
        periodic,
    }: Bound<Store, Guard>,
    disco: Disco,
) -> (
    impl FnOnce(),
    impl Future<Output = Result<Void, io::error::Accept>>,
)
where
    Store: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
    Guard: RequestPullGuard,
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

pub trait RequestPullGuard: request_pull::Guard + Clone + Send + Sync + 'static {}
impl<T> RequestPullGuard for T where T: request_pull::Guard + Clone + Send + Sync + 'static {}

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
