// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{net::SocketAddr, panic, time::Duration};

use futures::{future, StreamExt as _, TryFutureExt as _, TryStreamExt as _};
use futures_timer::Delay;
use thiserror::Error;
use tokio::task::spawn_blocking;

use super::protocol::{self, gossip};
use crate::{
    git::{self, Urn},
    signer::Signer,
    PeerId,
};

pub use super::protocol::{
    event::{downstream::Stats, Upstream as ProtocolEvent},
    PeerInfo,
};
pub use deadpool::managed::PoolError;

mod storage;
pub use storage::Storage as PeerStorage;

#[derive(Clone)]
pub struct Config<Signer> {
    pub signer: Signer,
    pub protocol: protocol::Config,
    pub storage_pools: PoolSizes,
}

#[derive(Clone, Copy)]
pub struct PoolSizes {
    /// Number of [`git::storage::Storage`] instances to pool for [`Peer`]
    /// consumers.
    ///
    /// Default: the number of physical cores available
    pub user: usize,

    /// Number of [`git::storage::Storage`] instances to reserve for protocol
    /// use.
    ///
    /// Default: the number of physical cores available
    pub protocol: usize,
}

impl Default for PoolSizes {
    fn default() -> Self {
        Self {
            user: num_cpus::get_physical(),
            protocol: num_cpus::get_physical(),
        }
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StorageError {
    #[error("the spawned blocking task was cancelled")]
    Cancelled,

    #[error(transparent)]
    Storage(#[from] git::storage::Error),

    #[error(transparent)]
    Pool(PoolError<git::storage::Error>),
}

impl From<PoolError<git::storage::Error>> for StorageError {
    fn from(e: PoolError<git::storage::Error>) -> Self {
        Self::Pool(e)
    }
}

#[derive(Clone)]
pub struct Peer<S> {
    config: Config<S>,
    phone: protocol::TinCans,
    peer_store: PeerStorage,
    git_store: git::storage::Pool,
}

impl<S> Peer<S>
where
    S: Signer + Clone,
{
    pub fn new(config: Config<S>) -> Self {
        let phone = protocol::TinCans::default();
        let peer_store = PeerStorage::new(git::storage::Pool::new(
            git::storage::pool::Config::new(config.protocol.paths.clone(), config.signer.clone()),
            config.storage_pools.protocol,
        ));
        let git_store = git::storage::Pool::new(
            git::storage::pool::Config::new(config.protocol.paths.clone(), config.signer.clone()),
            config.storage_pools.user,
        );

        Self {
            config,
            phone,
            peer_store,
            git_store,
        }
    }

    pub fn signer(&self) -> &S {
        &self.config.signer
    }

    pub fn peer_id(&self) -> PeerId {
        PeerId::from_signer(self.signer())
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
                Delay::new(timeout).await;
                Err("timed out")
            }),
            {
                let urn = urn.clone();
                events
                    .map_err(|_| "network reconnect")
                    .try_filter_map(move |event| {
                        let provider = match event {
                            Upstream::Gossip(box Gossip::Put {
                                provider,
                                payload:
                                    gossip::Payload {
                                        urn: payload_urn, ..
                                    },
                                ..
                            }) if payload_urn == urn => Some(provider),

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

    pub async fn stats(&self) -> Stats {
        self.phone.stats().await
    }

    pub fn subscribe(
        &self,
    ) -> impl futures::Stream<Item = Result<ProtocolEvent, protocol::RecvError>> {
        self.phone.subscribe()
    }

    pub async fn using_storage<F, A>(&self, blocking: F) -> Result<A, StorageError>
    where
        F: FnOnce(&git::storage::Storage) -> A + Send + 'static,
        A: Send + 'static,
    {
        let storage = self.git_store.get().await?;
        match spawn_blocking(move || blocking(&storage)).await {
            Ok(a) => Ok(a),
            Err(e) => {
                if e.is_cancelled() {
                    Err(StorageError::Cancelled)
                } else if e.is_panic() {
                    panic::resume_unwind(e.into_panic())
                } else {
                    panic!("unknown error awaiting spawned blocking task: {:?}", e)
                }
            },
        }
    }

    pub async fn storage(
        &self,
    ) -> Result<impl AsRef<git::storage::Storage>, PoolError<git::storage::Error>> {
        self.git_store
            .get()
            .map_ok(git::storage::pool::PooledRef::from)
            .await
    }

    pub async fn bind(&self) -> Result<protocol::Bound<PeerStorage>, protocol::error::Bootstrap> {
        protocol::bind(
            self.phone.clone(),
            self.config.protocol.clone(),
            self.config.signer.clone(),
            self.peer_store.clone(),
        )
        .await
    }
}

impl<S> git::local::transport::CanOpenStorage for Peer<S>
where
    S: Signer + Clone,
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
