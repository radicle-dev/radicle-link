// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use futures::{future, StreamExt as _, TryFutureExt as _, TryStreamExt as _};
use link_async::Spawner;

use crate::{
    git::{self, identities::local::LocalIdentity, Urn},
    net::{
        protocol::{self, gossip, TinCans},
        replication::{self, Replication},
    },
    PeerId,
    Signer,
};

pub use crate::net::protocol::{
    event::{
        self,
        downstream::{MembershipInfo, Stats},
        Upstream as ProtocolEvent,
    },
    rpc::client::{self, Client},
    Connected,
    Interrogation,
    PeerInfo,
    RequestPull,
    RequestPullGuard,
};

pub mod error;
pub mod storage;
pub use storage::Storage as PeerStorage;

#[derive(Clone)]
pub struct Config<Signer, Guard = config::DenyAll> {
    pub signer: Signer,
    pub protocol: protocol::Config<Guard>,
    pub storage: config::Storage,
}

pub mod config {
    pub use super::protocol::config::{Denied, DenyAll};

    #[derive(Clone, Copy, Default)]
    pub struct Storage {
        pub user: UserStorage,
        pub protocol: ProtocolStorage,
    }

    /// Settings for the user-facing storage.
    ///
    /// Cf. [`super::Peer::using_storage`]
    #[derive(Clone, Copy)]
    pub struct UserStorage {
        /// Number of [`crate::git::storage::Storage`] instances to reserve.
        pub pool_size: usize,
    }

    impl Default for UserStorage {
        fn default() -> Self {
            Self {
                pool_size: num_cpus::get_physical(),
            }
        }
    }

    /// Settings for the protocol storage.
    ///
    /// Cf. [`super::PeerStorage`]
    #[derive(Clone, Copy)]
    pub struct ProtocolStorage {
        /// Number of [`crate::git::storage::Storage`] instances to reserve.
        pub pool_size: usize,
    }

    impl Default for ProtocolStorage {
        fn default() -> Self {
            Self {
                pool_size: num_cpus::get_physical(),
            }
        }
    }
}

#[derive(Clone)]
pub struct Peer<S, G = config::DenyAll> {
    config: Config<S, G>,
    phone: protocol::TinCans,
    peer_store: PeerStorage,
    user_store: git::storage::Pool<git::storage::Storage>,
    caches: protocol::Caches,
    spawner: Arc<Spawner>,
    repl: Replication,
}

impl<S, G> Peer<S, G>
where
    S: Signer + Clone,
    G: RequestPullGuard,
{
    pub fn new(config: Config<S, G>) -> Result<Self, error::Init> {
        let spawner = Spawner::from_current()
            .map(Arc::new)
            .ok_or(error::Init::Runtime)?;
        let phone = protocol::TinCans::default();
        let storage_lock = git::storage::pool::Initialised::no();
        let pool = git::storage::Pool::new(
            git::storage::pool::ReadWriteConfig::new(
                config.protocol.paths.clone(),
                config.signer.clone(),
                storage_lock.clone(),
            ),
            config.storage.protocol.pool_size,
        );
        let caches = {
            let store = git::storage::Storage::open(&config.protocol.paths, config.signer.clone())?;
            let phone = phone.clone();
            let urns = protocol::cache::urns::Filter::new(store, move |ev| phone.emit(ev))?;
            protocol::Caches { urns }
        };

        let repl = Replication::new(&config.protocol.paths, config.protocol.replication)?;

        let peer_store = PeerStorage::new(
            storage::Config {
                fetch_quota: config.protocol.rate_limits.gossip.fetches_per_peer_and_urn,
            },
            spawner.clone(),
            pool,
            caches.urns.clone(),
            repl.clone(),
            phone.clone(),
        );
        let user_store = git::storage::Pool::new(
            git::storage::pool::ReadWriteConfig::new(
                config.protocol.paths.clone(),
                config.signer.clone(),
                storage_lock,
            ),
            config.storage.user.pool_size,
        );

        Ok(Self {
            config,
            phone,
            peer_store,
            user_store,
            caches,
            spawner,
            repl,
        })
    }

    pub fn signer(&self) -> &S {
        &self.config.signer
    }

    pub fn peer_id(&self) -> PeerId {
        PeerId::from_signer(self.signer())
    }

    pub fn protocol_config(&self) -> &protocol::Config<G> {
        &self.config.protocol
    }

    pub fn client(&self) -> Result<Client<S, TinCans>, client::error::Init> {
        let config = client::Config {
            user_storage: self.user_store.clone().into(),
            ..self.config.clone().into()
        };
        Client::new(config, self.spawner.clone(), self.phone.clone())
    }

    pub fn announce(&self, have: gossip::Payload) -> Result<(), gossip::Payload> {
        self.phone.announce(have)
    }

    pub fn query(&self, want: gossip::Payload) -> Result<(), gossip::Payload> {
        self.phone.query(want)
    }

    pub fn providers(
        &self,
        urn: Urn,
        timeout: Duration,
    ) -> impl futures::Stream<Item = PeerInfo<SocketAddr>> {
        use protocol::event::{upstream::Gossip, Upstream};

        let events = self.subscribe();
        let providers = futures::stream::select(
            futures::stream::once(async move {
                link_async::sleep(timeout).await;
                Err("timed out")
            }),
            {
                let urn = urn.clone();
                events
                    .map_err(|_| "network reconnect")
                    .try_filter_map(move |event| {
                        let provider = match event {
                            Upstream::Gossip(gossip) => match *gossip {
                                Gossip::Put {
                                    provider,
                                    payload:
                                        gossip::Payload {
                                            urn: payload_urn, ..
                                        },
                                    ..
                                } if payload_urn == urn => Some(provider),
                                _ => None,
                            },
                            _ => None,
                        };
                        future::ok(provider)
                    })
            },
        )
        .take_while(|x| future::ready(x.is_ok()))
        .map(Result::unwrap);

        match self.query(gossip::Payload {
            urn,
            rev: None,
            origin: None,
        }) {
            Ok(()) => providers.boxed(),
            Err(_) => futures::stream::empty().boxed(),
        }
    }

    pub async fn connected_peers(&self) -> Vec<PeerId> {
        self.phone.connected_peers().await
    }

    pub async fn membership(&self) -> MembershipInfo {
        self.phone.membership().await
    }

    pub async fn stats(&self) -> Stats {
        self.phone.stats().await
    }

    #[deprecated(
        note = "use of `self.interrogate(..)` is deprecated in favour of going through `self.client(..)?.interrogate(..)`"
    )]
    pub async fn interrogate(
        &self,
        from: impl Into<(PeerId, Vec<SocketAddr>)>,
    ) -> Result<Interrogation, error::NoConnection> {
        let from = from.into();
        let remote_peer = from.0;
        let Connected(conn) = self
            .connect(from)
            .await
            .ok_or(error::NoConnection(remote_peer))?;
        Ok(self.phone.interrogate(remote_peer, conn))
    }

    #[deprecated(
        note = "use of `self.request_pull(..)` is deprecated in favour of going through `self.client(..)?.request_pull(..)`"
    )]
    pub async fn request_pull<'a>(
        &self,
        to: impl Into<(PeerId, Vec<SocketAddr>)>,
        urn: Urn,
    ) -> Result<RequestPull, error::NoConnection> {
        let to = to.into();
        let remote_peer = to.0;
        let Connected(conn) = self
            .connect(to)
            .await
            .ok_or(error::NoConnection(remote_peer))?;
        Ok(self.phone.request_pull(urn, conn).await)
    }

    /// Initiate replication of `urn` from the given peer.
    ///
    /// If a connection to `from` does not already exist, the supplied addresses
    /// are used to establish a new one. It is legal to supply empty address
    /// hints so that only existing connections are used.
    ///
    /// `urn` may or may not already exist locally.
    ///
    /// The optional `whoami` parameter is used to advertise the identity the
    /// caller whishes to identify as, ie. the `rad/self` branch.
    #[deprecated(
        note = "use of `self.replicate(..)` is deprecated in favour of going through `self.client(..)?.replicate(..)`"
    )]
    pub async fn replicate(
        &self,
        from: impl Into<(PeerId, Vec<SocketAddr>)>,
        urn: Urn,
        whoami: Option<LocalIdentity>,
    ) -> Result<replication::Success, error::Replicate> {
        // TODO: errors
        let from = from.into();
        let remote_peer = from.0;
        let Connected(conn) = self
            .connect(from)
            .await
            .ok_or(error::Replicate::NoConnection(remote_peer))?;
        let store = self.user_store.get().await?;
        self.repl
            .replicate(&self.spawner, store, conn, urn, whoami)
            .err_into()
            .await
    }

    // TODO: Augment `Connected` such that we can provide an alternative API,
    // a la `peer.connect((peer_id, addrs)).await.unwrap().replicate()`
    async fn connect(&self, to: impl Into<(PeerId, Vec<SocketAddr>)>) -> Option<Connected> {
        self.phone.connect(to).await
    }

    pub fn subscribe(
        &self,
    ) -> impl futures::Stream<Item = Result<ProtocolEvent, protocol::RecvError>> {
        self.phone.subscribe()
    }

    /// Borrow a [`git::storage::Storage`] from the pool, and run a blocking
    /// computation on it.
    pub async fn using_storage<F, T>(&self, blocking: F) -> Result<T, error::Storage>
    where
        F: FnOnce(&git::storage::Storage) -> T + Send + 'static,
        T: Send + 'static,
    {
        let storage = self.user_store.get().await?;
        Ok(self.spawner.blocking(move || blocking(&storage)).await)
    }

    /// Borrow a [`git::storage::ReadOnly`] from the pool, and run a blocking
    /// computation on it.
    pub async fn using_read_only<F, T>(&self, blocking: F) -> Result<T, error::Storage>
    where
        F: FnOnce(&git::storage::ReadOnly) -> T + Send + 'static,
        T: Send + 'static,
    {
        let storage = self.user_store.get().await?;
        Ok(self
            .spawner
            .blocking(move || blocking(storage.read_only()))
            .await)
    }

    /// Borrow a [`git::storage::Storage`] from the pool directly.
    ///
    /// # WARNING
    ///
    /// Operations on [`git::storage::Storage`] are ususally blocking, and thus
    /// require to be spawned to a dedicated thread pool in an async
    /// context. [`Self::using_storage`] takes care of that, while the
    /// consumer of this method's return value is responsible for spawning
    /// themselves.
    ///
    /// Also note that the consumer is responsible for dropping the returned
    /// value in a timely fashion after it is no longer needed, in order to
    /// return the [`git::storage::Storage`] to the pool.
    pub async fn storage(
        &self,
    ) -> Result<impl AsRef<git::storage::Storage>, git::storage::pool::PoolError> {
        self.user_store
            .get()
            .map_ok(git::storage::pool::PooledRef::from)
            .await
    }

    pub async fn bind(
        &self,
    ) -> Result<protocol::Bound<PeerStorage, G>, protocol::error::Bootstrap> {
        protocol::bind(
            self.spawner.clone(),
            self.phone.clone(),
            self.config.protocol.clone(),
            self.config.signer.clone(),
            self.peer_store.clone(),
            self.caches.clone(),
        )
        .await
    }
}

impl<S, G> git::local::transport::CanOpenStorage for Peer<S, G>
where
    S: Signer + Clone,
    G: RequestPullGuard,
{
    fn open_storage(
        &self,
    ) -> Result<
        Box<dyn AsRef<git::storage::Storage>>,
        Box<dyn std::error::Error + Send + Sync + 'static>,
    > {
        match futures::executor::block_on(self.storage()) {
            Err(e) => Err(Box::new(e)),
            Ok(s) => Ok(Box::new(s)),
        }
    }
}
