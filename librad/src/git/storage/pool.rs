// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    ops::Deref,
    sync::{Arc, Mutex},
};

use deadpool::managed::{self, Manager, RecycleResult};

use super::{Error, Storage};
use crate::{paths::Paths, signer::Signer};

pub type Pool = deadpool::managed::Pool<Storage, Error>;
pub type PoolError = managed::PoolError<Error>;

#[async_trait]
pub trait Pooled {
    async fn get(&self) -> Result<PooledRef, PoolError>;
}

#[async_trait]
impl Pooled for Pool {
    async fn get(&self) -> Result<PooledRef, PoolError> {
        self.get().await.map(PooledRef::from)
    }
}

/// Wrapper so we can use [`Pooled`] as `AsRef<Storage>`.
// TODO: may go away once https://github.com/bikeshedder/deadpool/pull/69
// appears in a released version.
pub struct PooledRef(deadpool::managed::Object<Storage, Error>);

impl Deref for PooledRef {
    type Target = Storage;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl AsRef<Storage> for PooledRef {
    fn as_ref(&self) -> &Storage {
        self
    }
}

impl From<deadpool::managed::Object<Storage, Error>> for PooledRef {
    fn from(obj: deadpool::managed::Object<Storage, Error>) -> Self {
        Self(obj)
    }
}

#[derive(Clone)]
pub struct Config<S> {
    paths: Paths,
    signer: S,
    lock: Arc<Mutex<()>>,
}

impl<S> Config<S> {
    pub fn new(paths: Paths, signer: S) -> Self {
        Self {
            paths,
            signer,
            lock: Arc::new(Mutex::new(())),
        }
    }
}

#[async_trait]
impl<S> Manager<Storage, Error> for Config<S>
where
    S: Signer + Clone,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    async fn create(&self) -> Result<Storage, Error> {
        // FIXME(kim): we should `block_in_place` here, but that forces the
        // threaded runtime onto users
        let _lock = self.lock.lock().unwrap();
        {
            Storage::open_or_init(&self.paths, self.signer.clone())
        }
    }

    async fn recycle(&self, _: &mut Storage) -> RecycleResult<Error> {
        Ok(())
    }
}
