// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{net::SocketAddr, sync::Arc};

use crypto::peer::Originates;
use either::Either::{self, Left, Right};
use futures::TryFutureExt as _;
use git_ext::{self as ext, reference};
use link_async::Spawner;
use nonzero_ext::nonzero;

use crate::{
    git::{
        storage::{self, Pool, PoolError, PooledRef, ReadOnlyStorage as _},
        tracking,
        Urn,
    },
    identities::urn,
    net::{
        protocol::{broadcast, cache, gossip, Connected, TinCans},
        replication::{self, Replication},
    },
    rate_limit::{Keyed, RateLimiter},
    PeerId,
};

mod error;
pub use error::Error;

#[derive(Clone, Copy)]
pub struct Config {
    pub fetch_quota: governor::Quota,
}

#[derive(Clone)]
pub struct Storage {
    pool: Pool<storage::Storage>,
    urns: cache::urns::Filter,
    rate: Arc<RateLimiter<Keyed<(PeerId, Urn)>>>,
    exec: Arc<Spawner>,
    repl: Replication,
    tins: TinCans,
}

impl Storage {
    pub fn new(
        conf: Config,
        exec: Arc<Spawner>,
        pool: Pool<storage::Storage>,
        urns: cache::urns::Filter,
        repl: Replication,
        tins: TinCans,
    ) -> Self {
        Self {
            pool,
            urns,
            rate: Arc::new(RateLimiter::keyed(
                conf.fetch_quota,
                nonzero!(256 * 1024usize),
            )),
            exec,
            repl,
            tins,
        }
    }

    fn is_rate_limited(&self, remote_peer: PeerId, urn: Urn) -> bool {
        self.rate.check_key(&(remote_peer, urn)).is_err()
    }

    async fn git_fetch(
        &self,
        from: impl Into<(PeerId, Vec<SocketAddr>)>,
        urn: Either<Urn, Originates<Urn>>,
        head: impl Into<Option<git2::Oid>>,
    ) -> Result<replication::Success, Error> {
        if let Some(head) = head.into() {
            if self.git_has(urn.clone(), Some(head)).await {
                return Err(Error::KnownObject(head));
            }
        }

        let git = self.pool.get().await?;
        let urn = urn_context(*git.peer_id(), urn);
        let from = from.into();
        let remote_peer = from.0;
        if self.is_rate_limited(remote_peer, urn.clone().with_path(None)) {
            return Err(Error::RateLimited { remote_peer, urn });
        }

        match self.tins.connect(from).await {
            None => Err(Error::NoConnection { remote_peer }),
            Some(Connected(conn)) => {
                self.repl
                    .replicate(&self.exec, git, conn, urn, None)
                    .err_into::<Error>()
                    .await
            },
        }
    }

    /// Determine if we have the given object locally
    async fn git_has(
        &self,
        urn: Either<Urn, Originates<Urn>>,
        head: impl Into<Option<git2::Oid>>,
    ) -> bool {
        let git = self
            .pool
            .get()
            .await
            .expect("unable to acquire storage from pool");
        let urn = urn_context(*git.peer_id(), urn);

        if !self.urns.contains(&urn.clone().with_path(None).into()) {
            return false;
        }

        let head = head.into().map(ext::Oid::from);
        self.exec
            .blocking(move || match head {
                None => git.as_ref().has_urn(&urn).unwrap_or(false),
                Some(head) => {
                    git.as_ref().has_commit(&urn, head).unwrap_or(false)
                        || git.as_ref().has_tag(&urn, head).unwrap_or(false)
                },
            })
            .await
    }

    /// If the storage does not yet have the given `urn` *and* the default
    /// tracking entry exists, then the `urn` is considered tracked -- as we
    /// want to passively replicate the `urn`. Otherwise, the `urn` is only
    /// considered tracked if we have a tracked entry for the given `peer`.
    async fn is_tracked(&self, urn: Urn, peer: PeerId) -> Result<bool, Error> {
        let git = self.pool.get().await?;
        self.exec
            .blocking(move || -> Result<bool, Error> {
                Ok(tracking::is_tracked(git.as_ref(), &urn, Some(peer))?
                    || tracking::default_only(git.as_ref(), &urn)?)
            })
            .await
    }
}

/// If applicable, map the `path` of the given [`Urn`] to
/// `refs/remotes/<origin>/<path>`
pub fn urn_context(local_peer_id: PeerId, urn: Either<Urn, Originates<Urn>>) -> Urn {
    fn remote(urn: Urn, peer: PeerId) -> Urn {
        let path = reflike!("refs/remotes").join(peer).join(
            ext::RefLike::from(
                urn.path
                    .map(reference::Qualified::from)
                    .unwrap_or_else(|| urn::DEFAULT_PATH.clone()),
            )
            .strip_prefix("refs")
            .unwrap(),
        );

        Urn {
            id: urn.id,
            path: Some(path),
        }
    }

    fn local(urn: Urn) -> Urn {
        urn.map_path(|path| {
            path.map(reference::Qualified::from)
                .or_else(|| Some(urn::DEFAULT_PATH.clone()))
                .map(ext::RefLike::from)
        })
    }

    match urn {
        Left(urn) => local(urn),
        Right(Originates { from, value: urn }) if from == local_peer_id => local(urn),
        Right(Originates { from, value: urn }) => remote(urn, from),
    }
}

#[async_trait]
impl broadcast::LocalStorage<SocketAddr> for Storage {
    type Update = gossip::Payload;

    #[tracing::instrument(skip(self, provider))]
    async fn put<P>(&self, provider: P, has: Self::Update) -> broadcast::PutResult<Self::Update>
    where
        P: Into<(PeerId, Vec<SocketAddr>)> + Send,
    {
        use broadcast::PutResult;

        let (provider, addr_hints) = provider.into();

        // If the `has` doesn't tell us to look into a specific remote-tracking
        // branch, assume we want the `provider`'s.
        let origin = has.origin.unwrap_or(provider);
        let is_tracked = match self.is_tracked(has.urn.clone(), origin).await {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(err = %e, "error determining tracking status");
                return PutResult::Error;
            },
        };

        if is_tracked {
            let urn = Right(Originates {
                from: origin,
                value: has.urn.clone(),
            });
            let head = has.rev.as_ref().map(|gossip::Rev::Git(head)| *head);

            match self
                .git_fetch((provider, addr_hints), urn.clone(), head)
                .await
            {
                Ok(_) => {
                    // Verify that the announced data is stored locally now.
                    //
                    // If it is, rewrite the gossip message to use the `origin`
                    // we determined -- everyone down the line may now fetch
                    // the that remote from us.
                    //
                    // Otherwise, the `provider` must be lying -- we are
                    // tracking them, and there was no error, but the data is
                    // still not there. In this case, returning `Stale` will
                    // just terminate the broadcast here.
                    if self.git_has(urn, head).await {
                        PutResult::Applied(gossip::Payload {
                            origin: Some(origin),
                            ..has
                        })
                    } else {
                        tracing::warn!(
                            provider = %provider,
                            announced = ?has,
                            "provider announced non-existent rev"
                        );
                        PutResult::Stale
                    }
                },

                Err(e) => match e {
                    Error::KnownObject(_) => PutResult::Stale,
                    Error::RateLimited { remote_peer, urn } => {
                        tracing::warn!(
                            "skipped fetch of {} from {} due to rate limiting",
                            remote_peer,
                            urn
                        );
                        PutResult::Stale
                    },
                    x => {
                        tracing::error!(err = %x, "fetch error");
                        PutResult::Error
                    },
                },
            }
        } else {
            PutResult::Uninteresting
        }
    }

    #[tracing::instrument(level = "debug", skip(self))]
    async fn ask(&self, want: Self::Update) -> bool {
        self.git_has(
            match want.origin {
                Some(origin) => Right(Originates {
                    from: origin,
                    value: want.urn,
                }),
                None => Left(want.urn),
            },
            want.rev.map(|gossip::Rev::Git(head)| head),
        )
        .await
    }
}

#[async_trait]
impl storage::Pooled<storage::Storage> for Storage {
    async fn get(&self) -> Result<PooledRef<storage::Storage>, PoolError> {
        self.pool.get().await.map(PooledRef::from)
    }
}
