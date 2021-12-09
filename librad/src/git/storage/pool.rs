// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
    sync::Arc,
};

use deadpool::managed::{self, Manager, Object, RecycleResult};
use parking_lot::RwLock;
use thiserror::Error;

use super::{error, read, ReadOnly, Storage};
use crate::{paths::Paths, Signer};

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum InitError {
    #[error(transparent)]
    Read(#[from] read::error::Init),

    #[error(transparent)]
    Write(#[from] error::Init),
}

pub type Pool<S> = deadpool::managed::Pool<S, InitError>;
pub type PoolError = managed::PoolError<InitError>;

#[async_trait]
pub trait Pooled<S: Send> {
    async fn get(&self) -> Result<PooledRef<S>, PoolError>;
}

#[async_trait]
impl<S: Send> Pooled<S> for Pool<S> {
    async fn get(&self) -> Result<PooledRef<S>, PoolError> {
        self.get().await.map(PooledRef::from)
    }
}

/// A reference to a pooled storage.
///
/// The `S` parameter can be filled by [`Storage`] for read-write access or
/// [`ReadOnly`] for read-only access.
pub struct PooledRef<S>(Object<S, InitError>);

impl<S> Deref for PooledRef<S> {
    type Target = S;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<S> DerefMut for PooledRef<S> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

impl<S> AsRef<S> for PooledRef<S> {
    fn as_ref(&self) -> &S {
        self
    }
}

impl<S> AsMut<S> for PooledRef<S> {
    fn as_mut(&mut self) -> &mut S {
        self
    }
}

impl AsRef<ReadOnly> for PooledRef<Storage> {
    fn as_ref(&self) -> &ReadOnly {
        self.0.read_only()
    }
}

impl<S> From<Object<S, InitError>> for PooledRef<S> {
    fn from(obj: Object<S, InitError>) -> Self {
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

pub struct Write<S> {
    signer: S,
    init: Initialised,
}

#[derive(Clone)]
pub struct Config<W> {
    paths: Paths,
    write: W,
}

pub type ReadConfig = Config<PhantomData<!>>;
pub type ReadWriteConfig<S> = Config<Write<S>>;

impl ReadConfig {
    pub fn new(paths: Paths) -> Self {
        Config {
            paths,
            write: PhantomData,
        }
    }

    pub fn write<S>(self, signer: S, init: Initialised) -> ReadWriteConfig<S> {
        Config {
            paths: self.paths,
            write: Write { signer, init },
        }
    }
}

#[async_trait]
impl Manager<ReadOnly, InitError> for ReadConfig {
    async fn create(&self) -> Result<ReadOnly, InitError> {
        ReadOnly::open(&self.paths).map_err(InitError::from)
    }

    async fn recycle(&self, _: &mut ReadOnly) -> RecycleResult<InitError> {
        Ok(())
    }
}

impl<S> ReadWriteConfig<S> {
    pub fn new(paths: Paths, signer: S, init: Initialised) -> Self {
        Self {
            paths,
            write: Write { signer, init },
        }
    }

    fn mk_storage(&self) -> Result<Storage, InitError>
    where
        S: Signer + Clone,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        Storage::open(&self.paths, self.write.signer.clone()).map_err(InitError::from)
    }
}

#[async_trait]
impl<S> Manager<Storage, InitError> for ReadWriteConfig<S>
where
    S: Signer + Clone,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    async fn create(&self) -> Result<Storage, InitError> {
        let initialised = self.write.init.0.read();
        if *initialised {
            self.mk_storage()
        } else {
            drop(initialised);
            let mut initialised = self.write.init.0.write();
            self.mk_storage()
                .map(|storage| {
                    *initialised = true;
                    storage
                })
                .map_err(InitError::from)
        }
    }

    async fn recycle(&self, _: &mut Storage) -> RecycleResult<InitError> {
        Ok(())
    }
}
