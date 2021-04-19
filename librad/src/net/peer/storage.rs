// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{net::SocketAddr, time::Duration};

use either::Either::{self, Left, Right};
use git_ext::{self as ext, reference};
use tokio::task::spawn_blocking;

use crate::{
    git::{
        replication,
        storage::{self, fetcher, Pool, PoolError, PooledRef},
        tracking,
        Urn,
    },
    identities::urn,
    net::protocol::{broadcast, gossip},
    peer::{Originates, PeerId},
};

mod error;
pub use error::Error;

#[derive(Clone, Copy)]
pub struct Config {
    pub replication: replication::Config,
    pub fetch_slot_wait_timeout: Duration,
}

#[derive(Clone)]
pub struct Storage {
    pool: Pool,
    config: Config,
}

impl Storage {
    pub fn new(pool: Pool, config: Config) -> Self {
        Self { pool, config }
    }

    async fn git_fetch(
        &self,
        from: impl Into<(PeerId, Vec<SocketAddr>)>,
        urn: Either<Urn, Originates<Urn>>,
        head: impl Into<Option<git2::Oid>>,
    ) -> Result<replication::ReplicateResult, Error> {
        let urn = {
            let git = self.pool.get().await?;
            urn_context(*git.peer_id(), urn)
        };
        let head = head.into().map(ext::Oid::from);

        if let Some(head) = head {
            let git = self.pool.get().await?;
            let urn = urn.clone();
            spawn_blocking(move || {
                if git.has_commit(&urn, head)? || git.has_tag(&urn, head)? {
                    Err(Error::KnownObject(*head))
                } else {
                    Ok(())
                }
            })
            .await??;
        }

        let (remote_peer, addr_hints) = from.into();
        let config = self.config;
        fetcher::retrying(
            self.pool.clone(),
            fetcher::PeerToPeer::new(urn.clone(), remote_peer, addr_hints),
            config.fetch_slot_wait_timeout,
            move |storage, fetcher| {
                replication::replicate(storage, fetcher, config.replication, None)
                    .map_err(Error::from)
            },
        )
        .await?
    }

    /// Determine if we have the given object locally
    async fn git_has(
        &self,
        urn: Either<Urn, Originates<Urn>>,
        head: impl Into<Option<git2::Oid>>,
    ) -> bool {
        let git = self.pool.get().await.unwrap();
        let urn = urn_context(*git.peer_id(), urn);
        let head = head.into().map(ext::Oid::from);
        spawn_blocking(move || match head {
            None => git.has_urn(&urn).unwrap_or(false),
            Some(head) => {
                git.has_commit(&urn, head).unwrap_or(false)
                    || git.has_tag(&urn, head).unwrap_or(false)
            },
        })
        .await
        .unwrap_or(false)
    }

    async fn is_tracked(&self, urn: Urn, peer: PeerId) -> Result<bool, Error> {
        let git = self.pool.get().await?;
        Ok(spawn_blocking(move || tracking::is_tracked(&git, &urn, peer)).await??)
    }
}

/// If applicable, map the `path` of the given [`Urn`] to
/// `refs/remotes/<origin>/<path>`
fn urn_context(local_peer_id: PeerId, urn: Either<Urn, Originates<Urn>>) -> Urn {
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
impl storage::Pooled for Storage {
    async fn get(&self) -> Result<PooledRef, PoolError> {
        self.pool.get().await.map(PooledRef::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod urn_context {
        use super::*;
        use crate::keys::SecretKey;

        lazy_static! {
            static ref LOCAL_PEER_ID: PeerId = PeerId::from(SecretKey::from_seed([
                188, 124, 109, 100, 178, 93, 115, 53, 15, 22, 114, 181, 15, 211, 233, 104, 32, 189,
                9, 162, 235, 148, 204, 172, 21, 117, 34, 9, 236, 247, 238, 113
            ]));
            static ref OTHER_PEER_ID: PeerId = PeerId::from(SecretKey::from_seed([
                236, 225, 197, 234, 16, 153, 83, 54, 15, 203, 86, 253, 157, 81, 144, 96, 106, 99,
                65, 129, 8, 181, 125, 141, 120, 122, 58, 48, 22, 97, 32, 9
            ]));
            static ref ZERO_OID: ext::Oid = git2::Oid::zero().into();
        }

        #[test]
        fn direct_empty() {
            let urn = Urn::new(*ZERO_OID);
            let ctx = urn_context(*LOCAL_PEER_ID, Left(urn.clone()));
            assert_eq!(
                urn.with_path(ext::RefLike::from(urn::DEFAULT_PATH.clone())),
                ctx
            )
        }

        #[test]
        fn direct_onelevel() {
            let urn = Urn::new(*ZERO_OID).with_path(reflike!("ban/ana"));
            let ctx = urn_context(*LOCAL_PEER_ID, Left(urn.clone()));
            assert_eq!(urn.with_path(reflike!("refs/heads/ban/ana")), ctx)
        }

        #[test]
        fn direct_qualified() {
            let urn = Urn::new(*ZERO_OID).with_path(reflike!("refs/heads/next"));
            let ctx = urn_context(*LOCAL_PEER_ID, Left(urn.clone()));
            assert_eq!(urn, ctx)
        }

        #[test]
        fn remote_empty() {
            let urn = Urn::new(*ZERO_OID);
            let ctx = urn_context(
                *LOCAL_PEER_ID,
                Right(Originates {
                    from: *OTHER_PEER_ID,
                    value: urn.clone(),
                }),
            );
            assert_eq!(
                urn.with_path(
                    reflike!("refs/remotes").join(*OTHER_PEER_ID).join(
                        ext::RefLike::from(urn::DEFAULT_PATH.clone())
                            .strip_prefix("refs")
                            .unwrap()
                    )
                ),
                ctx
            )
        }

        #[test]
        fn remote_onelevel() {
            let urn = Urn::new(*ZERO_OID).with_path(reflike!("ban/ana"));
            let ctx = urn_context(
                *LOCAL_PEER_ID,
                Right(Originates {
                    from: *OTHER_PEER_ID,
                    value: urn.clone(),
                }),
            );
            assert_eq!(
                urn.with_path(
                    reflike!("refs/remotes")
                        .join(*OTHER_PEER_ID)
                        .join(reflike!("heads/ban/ana"))
                ),
                ctx
            )
        }

        #[test]
        fn remote_qualified() {
            let urn = Urn::new(*ZERO_OID).with_path(reflike!("refs/heads/next"));
            let ctx = urn_context(
                *LOCAL_PEER_ID,
                Right(Originates {
                    from: *OTHER_PEER_ID,
                    value: urn.clone(),
                }),
            );
            assert_eq!(
                urn.with_path(
                    reflike!("refs/remotes")
                        .join(*OTHER_PEER_ID)
                        .join(reflike!("heads/next"))
                ),
                ctx
            )
        }

        #[test]
        fn self_origin_empty() {
            let urn = Urn::new(*ZERO_OID);
            let ctx = urn_context(
                *LOCAL_PEER_ID,
                Right(Originates {
                    from: *LOCAL_PEER_ID,
                    value: urn.clone(),
                }),
            );
            assert_eq!(
                urn.with_path(ext::RefLike::from(urn::DEFAULT_PATH.clone())),
                ctx
            )
        }

        #[test]
        fn self_origin_onelevel() {
            let urn = Urn::new(*ZERO_OID).with_path(reflike!("ban/ana"));
            let ctx = urn_context(
                *LOCAL_PEER_ID,
                Right(Originates {
                    from: *LOCAL_PEER_ID,
                    value: urn.clone(),
                }),
            );
            assert_eq!(urn.with_path(reflike!("refs/heads/ban/ana")), ctx)
        }

        #[test]
        fn self_origin_qualified() {
            let urn = Urn::new(*ZERO_OID).with_path(reflike!("refs/heads/next"));
            let ctx = urn_context(
                *LOCAL_PEER_ID,
                Right(Originates {
                    from: *LOCAL_PEER_ID,
                    value: urn.clone(),
                }),
            );
            assert_eq!(urn, ctx)
        }
    }
}
