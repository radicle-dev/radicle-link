// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    ops::Deref,
    sync::{Arc, Weak},
    time::{Duration, SystemTime},
};

use futures_timer::Delay;
use parking_lot::{RwLock, RwLockReadGuard};
use thiserror::Error;

use crate::{
    executor,
    git::{identities, storage, tracking},
    identities::{xor, SomeUrn, Xor},
};

#[derive(Clone)]
pub struct Caches {
    pub urns: urns::Filter,
}

pub mod urns {
    use super::*;

    #[derive(Debug, Error)]
    #[non_exhaustive]
    pub enum Error {
        #[error("cache is initialising")]
        Initialising,
    }

    #[derive(Clone)]
    pub struct Filter {
        inner: Arc<RwLock<Option<FilterInner>>>,
    }

    struct FilterInner {
        modified: SystemTime,
        filter: Xor,
    }

    impl Filter {
        pub fn new<S>(spawner: Arc<executor::Spawner>, pool: S) -> Self
        where
            S: storage::Pooled + Clone + Send + Sync + 'static,
        {
            let inner = Arc::new(RwLock::new(None));
            spawner
                .spawn(refresh(Arc::clone(&spawner), pool, Arc::downgrade(&inner)))
                .detach();
            Self { inner }
        }

        pub fn contains(&self, urn: &SomeUrn) -> Result<bool, Error> {
            match &*self.inner.read() {
                None => Err(Error::Initialising),
                Some(inner) => Ok(inner.filter.contains(urn)),
            }
        }

        pub fn get(&self) -> Result<impl Deref<Target = Xor> + '_, Error> {
            let guard = self.inner.read();
            match *guard {
                None => Err(Error::Initialising),
                Some(_) => Ok(RwLockReadGuard::map(guard, |x| {
                    x.as_ref().map(|inner| &inner.filter).unwrap()
                })),
            }
        }
    }

    #[derive(Debug, Error)]
    enum RefreshError {
        #[error(transparent)]
        Tracking(#[from] tracking::Error),

        #[error(transparent)]
        Xor(#[from] xor::BuildError<identities::Error>),

        #[error(transparent)]
        Pool(#[from] storage::PoolError),

        #[error(transparent)]
        Task(#[from] executor::JoinError),
    }

    #[tracing::instrument(skip(spawner, pool, filter))]
    async fn refresh<S>(
        spawner: Arc<executor::Spawner>,
        pool: S,
        filter: Weak<RwLock<Option<FilterInner>>>,
    ) where
        S: storage::Pooled + Send + Sync,
    {
        async fn mtime<S>(spawner: &executor::Spawner, pool: &S) -> Result<SystemTime, RefreshError>
        where
            S: storage::Pooled + Send + Sync,
        {
            let storage = pool.get().await?;
            Ok(spawner
                .spawn_blocking(move || tracking::modified(&storage))
                .await??)
        }

        async fn should_rebuild<S>(
            spawner: &executor::Spawner,
            pool: &S,
            filter: &RwLock<Option<FilterInner>>,
        ) -> Result<bool, RefreshError>
        where
            S: storage::Pooled + Send + Sync,
        {
            let modified = (&*filter.read()).as_ref().map(|inner| inner.modified);
            match modified {
                None => Ok(true),
                Some(t) => {
                    let mtime = mtime(spawner, pool).await?;
                    Ok(mtime > t)
                },
            }
        }

        async fn do_rebuild<S>(spawner: &executor::Spawner, pool: &S) -> Result<Xor, RefreshError>
        where
            S: storage::Pooled + Send + Sync,
        {
            let storage = pool.get().await?;
            Ok(spawner
                .spawn_blocking(move || identities::any::xor_filter(&storage))
                .await??)
        }

        loop {
            match Weak::upgrade(&filter) {
                None => break,
                Some(lock) => {
                    let should_rebuild = should_rebuild(&spawner, &pool, &lock)
                        .await
                        .unwrap_or_else(|e| {
                            tracing::warn!(err = ?e, "unable to determine freshness");
                            false
                        });
                    if should_rebuild {
                        tracing::info!("rebuilding xor filter");
                        match do_rebuild(&spawner, &pool).await {
                            Err(e) => tracing::warn!(err = ?e, "error rebuilding xor filter"),
                            Ok(xor) => {
                                tracing::info!("rebuilt xor filter");
                                let modified = mtime(&spawner, &pool)
                                    .await
                                    .unwrap_or_else(|_| SystemTime::now());
                                let mut guard = lock.write();
                                *guard = Some(FilterInner {
                                    modified,
                                    filter: xor,
                                })
                            },
                        }
                    }

                    Delay::new(Duration::from_secs(120)).await
                },
            }
        }
    }
}
