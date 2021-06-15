// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

use deadpool::managed::{self, Manager, Object, RecycleResult};
use parking_lot::RwLock;

use super::{error, Fetchers, ReadOnly, Storage};
use crate::{paths::Paths, signer::Signer};

pub type Pool = deadpool::managed::Pool<Storage, error::Init>;
pub type PoolError = managed::PoolError<error::Init>;

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

/// A reference to a pooled [`Storage`].
pub struct PooledRef(Object<Storage, error::Init>);

impl Deref for PooledRef {
    type Target = Storage;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for PooledRef {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

impl AsRef<Storage> for PooledRef {
    fn as_ref(&self) -> &Storage {
        self
    }
}

impl AsMut<Storage> for PooledRef {
    fn as_mut(&mut self) -> &mut Storage {
        self
    }
}

impl AsRef<ReadOnly> for PooledRef {
    fn as_ref(&self) -> &ReadOnly {
        self.read_only()
    }
}

impl From<Object<Storage, error::Init>> for PooledRef {
    fn from(obj: Object<Storage, error::Init>) -> Self {
        Self(obj)
    }
}

#[derive(Clone)]
pub struct Initialised(Arc<RwLock<bool>>);

impl Initialised {
    pub fn no() -> Self {
        Self(Arc::new(RwLock::new(false)))
    }
}

#[derive(Clone)]
pub struct Config<S> {
    paths: Paths,
    signer: S,
    init: Initialised,
    fetchers: Fetchers,
}

impl<S> Config<S> {
    pub fn new(paths: Paths, signer: S, init: Initialised) -> Self {
        Self::with_fetchers(paths, signer, init, Default::default())
    }

    pub fn with_fetchers(paths: Paths, signer: S, init: Initialised, fetchers: Fetchers) -> Self {
        Self {
            paths,
            signer,
            init,
            fetchers,
        }
    }
}

impl<S> Config<S>
where
    S: Signer + Clone,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    fn mk_storage(&self) -> Result<Storage, error::Init> {
        Storage::with_fetchers(&self.paths, self.signer.clone(), self.fetchers.clone())
    }
}

#[async_trait]
impl<S> Manager<Storage, error::Init> for Config<S>
where
    S: Signer + Clone,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    async fn create(&self) -> Result<Storage, error::Init> {
        let initialised = self.init.0.read();
        if *initialised {
            self.mk_storage()
        } else {
            drop(initialised);
            let mut initialised = self.init.0.write();
            self.mk_storage().map(|storage| {
                *initialised = true;
                storage
            })
        }
    }

    async fn recycle(&self, _: &mut Storage) -> RecycleResult<error::Init> {
        Ok(())
    }
}
