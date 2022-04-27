// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{net::SocketAddr, sync::Arc};

use crypto::Signer;

use futures::TryFutureExt;

use link_async::Spawner;

use crate::{
    git::{self, identities::local::LocalIdentity, Urn},
    net::{
        quic::ConnectPeer,
        replication::{self, Replication},
    },
    paths::Paths,
    PeerId,
};

pub mod config;
pub use config::Config;
pub mod error;

mod interrogation;
pub use interrogation::Interrogation;
mod request_pull;
pub use request_pull::RequestPull;

mod streams;

#[derive(Clone)]
pub struct Client<Signer, Endpoint: Clone + Send + Sync> {
    config: Config<Signer>,
    local_id: PeerId,
    spawner: Arc<Spawner>,
    paths: Arc<Paths>,
    endpoint: Endpoint,
    repl: Replication,
    user_store: git::storage::Pool<git::storage::Storage>,
}

impl<S, E: Clone + Send + Sync> Client<S, E>
where
    S: Signer + Clone,
{
    pub fn new(config: Config<S>, spawner: Arc<Spawner>, endpoint: E) -> Result<Self, error::Init> {
        let paths = config.paths.clone();
        let local_id = PeerId::from_signer(&config.signer);
        let user_store = config.storage();
        #[cfg(feature = "replication-v3")]
        let repl = Replication::new(&paths, config.replication)?;
        #[cfg(not(feature = "replication-v3"))]
        let repl = Replication::new(config.replication);

        Ok(Self {
            config,
            local_id,
            spawner,
            paths: Arc::new(paths),
            endpoint,
            repl,
            user_store,
        })
    }
}

impl<S, E> Client<S, E>
where
    S: Signer + Clone,
    E: ConnectPeer + Clone + Send + Sync + 'static,
{
    pub fn paths(&self) -> &Paths {
        &self.config.paths
    }

    pub fn peer_id(&self) -> PeerId {
        self.local_id
    }

    pub async fn replicate(
        &self,
        from: impl Into<(PeerId, Vec<SocketAddr>)>,
        urn: Urn,
        whoami: Option<LocalIdentity>,
    ) -> Result<replication::Success, error::Replicate> {
        #[cfg(feature = "replication-v3")]
        {
            // TODO: errors
            let (remote_peer, addrs) = from.into();
            let conn = self
                .endpoint
                .connect(remote_peer, addrs)
                .await
                .ok_or(error::NoConnection(remote_peer))?
                .connection()
                .clone();
            let store = self.user_store.get().await?;
            self.repl
                .replicate(&self.spawner, store, conn, urn, whoami)
                .err_into()
                .await
        }
        #[cfg(not(feature = "replication-v3"))]
        {
            self.repl
                .replicate(&self.spawner, &self.user_store, from, urn, whoami)
                .err_into()
                .await
        }
    }

    pub async fn request_pull(
        &self,
        to: impl Into<(PeerId, Vec<SocketAddr>)>,
        urn: Urn,
    ) -> Result<RequestPull, error::RequestPull> {
        let (remote_peer, addrs) = to.into();

        let ingress = self
            .endpoint
            .connect(remote_peer, addrs)
            .await
            .ok_or(error::NoConnection(remote_peer))?;
        let (conn, incoming) = match ingress {
            crate::net::quic::Ingress::Remote(conn) => (conn, None),
            crate::net::quic::Ingress::Local { conn, streams } => (conn, Some(streams)),
        };

        RequestPull::new(conn, incoming, urn, self.paths.clone()).await
    }

    pub async fn interrogate(
        &self,
        from: impl Into<(PeerId, Vec<SocketAddr>)>,
    ) -> Result<Interrogation, error::NoConnection> {
        let (remote_peer, addrs) = from.into();
        let ingress = self
            .endpoint
            .connect(remote_peer, addrs)
            .await
            .ok_or(error::NoConnection(remote_peer))?;

        Ok(Interrogation {
            peer: remote_peer,
            conn: ingress.connection().clone(),
        })
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
}
