// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{
    crypto::Signer,
    git::{
        self,
        storage::pool::{self, Pool},
    },
    net::{peer, protocol::replication, Network},
    paths::Paths,
};

#[derive(Clone)]
pub struct Config<Signer> {
    pub signer: Signer,
    pub paths: Paths,
    pub replication: replication::Config,
    pub user_storage: Storage,
    pub network: Network,
}

impl<S: Clone + Signer> Config<S> {
    pub fn storage(&self) -> Pool<git::storage::Storage> {
        match &self.user_storage {
            Storage::New(user_storage) => Pool::new(
                pool::ReadWriteConfig::new(
                    self.paths.clone(),
                    self.signer.clone(),
                    pool::Initialised::no(),
                ),
                user_storage.pool_size,
            ),
            Storage::Initialised(pool) => pool.clone(),
        }
    }
}

impl<S, G> From<peer::Config<S, G>> for Config<S> {
    fn from(config: peer::Config<S, G>) -> Self {
        Self {
            signer: config.signer,
            paths: config.protocol.paths,
            replication: config.protocol.replication,
            user_storage: UserStorage::from(config.storage.user).into(),
            network: config.protocol.network,
        }
    }
}

#[derive(Clone)]
pub enum Storage {
    New(UserStorage),
    Initialised(Pool<git::storage::Storage>),
}

impl Default for Storage {
    fn default() -> Self {
        Self::New(Default::default())
    }
}

impl From<UserStorage> for Storage {
    fn from(x: UserStorage) -> Self {
        Self::New(x)
    }
}

impl From<Pool<git::storage::Storage>> for Storage {
    fn from(x: Pool<git::storage::Storage>) -> Self {
        Self::Initialised(x)
    }
}

#[derive(Clone, Debug)]
pub struct UserStorage {
    pub pool_size: usize,
}

impl From<peer::config::UserStorage> for UserStorage {
    fn from(peer::config::UserStorage { pool_size }: peer::config::UserStorage) -> Self {
        Self { pool_size }
    }
}

impl Default for UserStorage {
    fn default() -> Self {
        Self {
            pool_size: num_cpus::get_physical(),
        }
    }
}
