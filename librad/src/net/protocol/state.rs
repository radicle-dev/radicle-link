// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{net::SocketAddr, ops::Deref, sync::Arc};

use link_async::Spawner;
use nonzero_ext::nonzero;
use rand_pcg::Pcg64Mcg;
use tracing::Instrument as _;

use super::{
    broadcast,
    cache,
    event,
    gossip,
    membership,
    request_pull,
    tick,
    Endpoint,
    ProtocolStorage,
    RequestPullGuard,
    TinCans,
};
use crate::{
    git::storage::{self, PoolError, PooledRef},
    net::quic,
    paths::Paths,
    rate_limit::{self, Direct, Keyed, RateLimiter},
    PeerId,
};

#[derive(Clone)]
pub(super) struct StateConfig {
    pub paths: Arc<Paths>,
}

/// Runtime state of a protocol instance.
///
/// You know, like `ReaderT (State s) IO`.
#[derive(Clone)]
pub(super) struct State<S, G> {
    pub local_id: PeerId,
    pub endpoint: Endpoint,
    pub membership: membership::Hpv<Pcg64Mcg, SocketAddr>,
    pub gossip: broadcast::State<Storage<S>, ()>,
    pub request_pull: request_pull::State<Storage<S>, G>,
    pub phone: TinCans,
    pub config: StateConfig,
    pub caches: cache::Caches,
    pub spawner: Arc<Spawner>,
    pub limits: RateLimits,
}

impl<S, G> State<S, G> {
    pub fn emit<I, E>(&self, evs: I)
    where
        I: IntoIterator<Item = E>,
        E: Into<event::Upstream>,
    {
        for evt in evs {
            self.phone.emit(evt)
        }
    }
}

impl<S, G> State<S, G>
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
    G: RequestPullGuard,
{
    pub async fn tick<I>(&self, tocks: I)
    where
        I: IntoIterator<Item = tick::Tock<SocketAddr, gossip::Payload>>,
    {
        for tock in tocks {
            tick::tock(self.clone(), tock).await
        }
    }

    /// Get or establish a connection
    ///
    /// Note: this function cannot be used in any of the
    /// `net::protocol::recv::*` modules, since `net::protocol::io::streams`
    /// relies on those modules and cycle will be created.
    pub async fn connection<I>(&self, to: PeerId, addr_hints: I) -> Option<quic::Connection>
    where
        I: IntoIterator<Item = SocketAddr> + 'static,
    {
        use super::io;

        match self.endpoint.get_connection(to) {
            Some(conn) => Some(conn),
            None => io::connect(&self.endpoint, to, addr_hints)
                .in_current_span()
                .await
                .map(|(conn, ingress)| {
                    self.spawner
                        .spawn(io::streams::incoming(self.clone(), ingress))
                        .detach();
                    conn
                }),
        }
    }

    pub fn has_connection(&self, to: PeerId) -> bool {
        self.endpoint.get_connection(to).is_some()
    }
}

#[cfg(not(feature = "replication-v3"))]
#[async_trait]
impl<S, G> crate::git::p2p::transport::GitStreamFactory for State<S, G>
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
    G: RequestPullGuard,
{
    async fn open_stream(
        &self,
        to: &PeerId,
        addr_hints: &[SocketAddr],
    ) -> Option<Box<dyn crate::git::p2p::transport::GitStream>> {
        use crate::net::upgrade;
        use futures::TryFutureExt as _;

        let span = tracing::info_span!("open-git-stream", remote_id = %to);
        match self
            .connection(*to, addr_hints.to_vec())
            .instrument(span.clone())
            .await
        {
            None => {
                span.in_scope(|| tracing::error!("unable to obtain connection"));
                None
            },

            Some(conn) => {
                let stream = conn
                    .open_bidi()
                    .inspect_err(|e| tracing::error!(err = ?e, "unable to open stream"))
                    .instrument(span.clone())
                    .await
                    .ok()?;
                let upgraded = upgrade::upgrade(stream, upgrade::Git)
                    .inspect_err(|e| tracing::error!(err = ?e, "unable to upgrade stream"))
                    .instrument(span)
                    .await
                    .ok()?;

                Some(Box::new(upgraded))
            },
        }
    }
}

//
// Rate Limiting
//

#[derive(Clone)]
pub(super) struct RateLimits {
    pub membership: Arc<RateLimiter<Keyed<PeerId>>>,
}

/// Rate limit quota.
#[derive(Clone, Debug)]
pub struct Quota {
    /// See [`GossipQuota`].
    pub gossip: GossipQuota,
    /// Membership messages per peer.
    ///
    /// When a peer sends membership messages at a higher rate, it will be
    /// disconnected.
    ///
    /// Default: 1/sec (burst: 10)
    pub membership: rate_limit::Quota,
    /// See [`StorageQuota`].
    pub storage: StorageQuota,
}

impl Default for Quota {
    fn default() -> Self {
        Self {
            gossip: GossipQuota::default(),
            membership: rate_limit::Quota::per_second(nonzero!(1u32)).allow_burst(nonzero!(10u32)),
            storage: StorageQuota::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct GossipQuota {
    /// Fetch attempts per peer and Urn.
    ///
    /// Default: 1/min (burst: 5)
    pub fetches_per_peer_and_urn: rate_limit::Quota,
}

impl Default for GossipQuota {
    fn default() -> Self {
        Self {
            fetches_per_peer_and_urn: rate_limit::Quota::per_minute(nonzero!(1u32))
                .allow_burst(nonzero!(5u32)),
        }
    }
}

/// Peer storage quota.
#[derive(Clone, Debug)]
pub struct StorageQuota {
    /// Local storage errors to tolerate.
    ///
    /// While the limit is not breached, applying the gossip message to local
    /// storage will be retried. The quota should be rather low, as storage
    /// errors are generally not expected to be transient.
    ///
    /// Default: 10/min
    pub errors: rate_limit::Quota,
    /// `Want` requests to respond to per remote peer.
    ///
    /// When this limit is breached, `Want`s from the peer will be ignored.
    ///
    /// Default: 30/min
    pub wants: rate_limit::Quota,
}

impl Default for StorageQuota {
    fn default() -> Self {
        Self {
            errors: rate_limit::Quota::per_minute(nonzero!(10u32)),
            wants: rate_limit::Quota::per_minute(nonzero!(30u32)),
        }
    }
}

//
// Peer Storage (gossip)
//

#[derive(Clone)]
struct StorageLimits {
    errors: Arc<RateLimiter<Direct>>,
    wants: Arc<RateLimiter<Keyed<PeerId>>>,
}

#[derive(Clone)]
pub(super) struct Storage<S> {
    inner: S,
    limits: StorageLimits,
}

impl<S> Storage<S> {
    pub fn new(inner: S, quota: StorageQuota) -> Self {
        Self {
            inner,
            limits: StorageLimits {
                errors: Arc::new(RateLimiter::direct(quota.errors)),
                wants: Arc::new(RateLimiter::keyed(quota.wants, nonzero!(256 * 1024usize))),
            },
        }
    }
}

impl<S> Deref for Storage<S> {
    type Target = S;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[async_trait]
impl<A, S> broadcast::LocalStorage<A> for Storage<S>
where
    A: 'static,
    S: broadcast::LocalStorage<A>,
    S::Update: Send,
{
    type Update = S::Update;

    async fn put<P>(&self, provider: P, has: Self::Update) -> broadcast::PutResult<Self::Update>
    where
        P: Into<(PeerId, Vec<A>)> + Send,
    {
        self.inner.put(provider, has).await
    }

    async fn ask(&self, want: Self::Update) -> bool {
        self.inner.ask(want).await
    }
}

impl<S> broadcast::RateLimited for Storage<S> {
    fn is_rate_limit_breached(&self, lim: broadcast::Limit) -> bool {
        use broadcast::Limit;

        match lim {
            Limit::Errors => self.limits.errors.check().is_err(),
            Limit::Wants { recipient } => self.limits.wants.check_key(recipient).is_err(),
        }
    }
}

#[async_trait]
impl<S> storage::Pooled<storage::Storage> for Storage<S>
where
    S: storage::Pooled<storage::Storage> + Send + Sync,
{
    async fn get(&self) -> Result<PooledRef<storage::Storage>, PoolError> {
        self.inner.get().await
    }
}
