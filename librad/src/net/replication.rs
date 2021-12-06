// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{sync::Arc, time::Duration};

use async_lock::Semaphore;
use link_async::{timeout, Spawner};
use link_replication::io::UserInfo;
use tracing::debug;

use crate::{
    git::{
        identities::local::LocalIdentity,
        storage::{read::ReadOnlyStorage as _, Storage},
    },
    identities::git::Urn,
    net::{connection::RemotePeer as _, quic},
    paths::Paths,
    PeerId,
};

pub use link_replication::FetchLimit;

mod context;
use context::Context;

pub mod error {
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum Init {
        #[error("failed to open object database")]
        Odb(#[source] link_replication::Error),

        #[error("failed to open reference database")]
        Refdb(#[from] link_git::refs::db::error::Open),
    }

    #[derive(Debug, Error)]
    pub enum Replicate {
        #[error("timeout waiting for replication slot")]
        Timeout(#[from] link_async::Elapsed),

        #[error(transparent)]
        Replicate(#[from] link_replication::Error),
    }
}

pub type Success = link_replication::Success<context::Urn>;

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub limit: FetchLimit,
    pub slots: usize,
    pub wait_slot: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            limit: FetchLimit::default(),
            slots: 4,
            wait_slot: Duration::from_secs(5),
        }
    }
}

#[derive(Clone)]
pub struct Replication {
    config: Config,
    slots: Arc<Semaphore>,
    odb: link_replication::io::Odb,
    rdb: link_git::refs::db::Refdb,
}

impl Replication {
    pub fn new(paths: &Paths, config: Config) -> Result<Self, error::Init> {
        let slots = Arc::new(Semaphore::new(config.slots));
        let odb = link_replication::io::Odb::open(paths.git_dir()).map_err(error::Init::Odb)?;
        let rdb = link_git::refs::db::Refdb::open(paths.git_dir())?;

        Ok(Self {
            config,
            slots,
            odb,
            rdb,
        })
    }

    pub async fn replicate<S>(
        &self,
        spawner: &Spawner,
        store: S,
        conn: quic::Connection,
        urn: Urn,
        whoami: Option<LocalIdentity>,
    ) -> Result<Success, error::Replicate>
    where
        S: AsRef<Storage> + Send + 'static,
    {
        let slot = timeout(self.config.wait_slot, self.slots.acquire_arc()).await?;
        let limit = self.config.limit;
        let odb = self.odb.clone();
        let rdb = self.rdb.clone();
        let res = spawner
            .blocking(move || {
                let store = store.as_ref();
                let have_urn = store.has_urn(&urn)?;
                let remote_id = conn.remote_peer_id();
                let info = UserInfo {
                    name: store.config()?.user_name()?,
                    peer_id: *store.peer_id(),
                };
                let urn = context::Urn::from(urn);
                let refdb = link_replication::io::Refdb::new(info, odb.clone(), rdb.clone(), &urn)?;
                let net = link_replication::io::Network::new(
                    refdb.clone(),
                    conn,
                    store.path(),
                    urn.clone(),
                );
                let mut cx = Context {
                    urn,
                    store,
                    refdb,
                    net,
                };
                let whoami = whoami.map(|id| link_replication::LocalIdentity {
                    tip: id.content_id.into(),
                    ids: id
                        .delegations()
                        .into_iter()
                        .copied()
                        .map(PeerId::from)
                        .collect(),
                });

                if have_urn {
                    debug!("pull");
                    link_replication::pull(&mut cx, limit, remote_id, whoami)
                } else {
                    debug!("clone");
                    link_replication::clone(&mut cx, limit, remote_id, whoami)
                }
            })
            .await
            .map_err(error::Replicate::Replicate);
        drop(slot);
        res
    }
}
