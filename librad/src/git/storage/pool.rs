// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::sync::{Arc, Mutex};

use deadpool::managed::{Manager, RecycleResult};

use super::{Error, Storage};
use crate::{keys, paths::Paths, signer::Signer};

pub type Pool<S> = deadpool::managed::Pool<Storage<S>, Error>;
pub type Pooled<S> = deadpool::managed::Object<Storage<S>, Error>;

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
impl<S> Manager<Storage<S>, Error> for Config<S>
where
    S: Signer + Clone,
    S::Error: keys::SignError,
{
    async fn create(&self) -> Result<Storage<S>, Error> {
        // FIXME(kim): we should `block_in_place` here, but that forces the
        // threaded runtime onto users
        let _lock = self.lock.lock().unwrap();
        {
            Storage::open_or_init(&self.paths, self.signer.clone())
        }
    }

    async fn recycle(&self, _: &mut Storage<S>) -> RecycleResult<Error> {
        Ok(())
    }
}
