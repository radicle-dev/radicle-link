// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

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
