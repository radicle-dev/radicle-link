// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use thiserror::Error;

use crate::{executor, git::storage, net::protocol::cache};

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Storage {
    #[error(transparent)]
    Task(executor::Cancelled),

    #[error(transparent)]
    Storage(#[from] storage::Error),

    #[error(transparent)]
    Pool(storage::PoolError),
}

impl From<storage::PoolError> for Storage {
    fn from(e: storage::PoolError) -> Self {
        Self::Pool(e)
    }
}

impl From<executor::JoinError> for Storage {
    fn from(e: executor::JoinError) -> Self {
        Self::Task(e.into_cancelled())
    }
}

#[derive(Debug, Error)]
pub enum Init {
    #[error(transparent)]
    Storage(#[from] storage::Error),

    #[error(transparent)]
    Cache(#[from] Box<cache::urns::Error>),
}

impl From<cache::urns::Error> for Init {
    fn from(e: cache::urns::Error) -> Self {
        Self::from(Box::new(e))
    }
}
