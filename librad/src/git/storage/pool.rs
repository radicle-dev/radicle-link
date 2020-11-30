// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::sync::{Arc, Mutex};

use deadpool::managed::{Manager, RecycleResult};

use super::{Error, Storage};
use crate::{paths::Paths, signer::Signer};

pub type Pool = deadpool::managed::Pool<Storage, Error>;
pub type Pooled = deadpool::managed::Object<Storage, Error>;

/// Wrapper so we can use [`Pooled`] as `AsRef<Storage>`.
// TODO: may go away once https://github.com/bikeshedder/deadpool/pull/69
// appears in a released version.
pub struct PooledRef(Pooled);

impl AsRef<Storage> for PooledRef {
    fn as_ref(&self) -> &Storage {
        &self.0
    }
}

impl From<Pooled> for PooledRef {
    fn from(pooled: Pooled) -> Self {
        Self(pooled)
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
