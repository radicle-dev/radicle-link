// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use thiserror::Error;

use crate::{
    executor,
    git::{self, replication, storage::fetcher, tracking},
};

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("already have {0}")]
    KnownObject(git2::Oid),

    #[error(transparent)]
    Tracking(#[from] tracking::Error),

    #[error(transparent)]
    Replication(#[from] replication::Error),

    #[error("unable to obtain fetcher")]
    Fetcher(#[from] fetcher::error::Retrying<git2::Error>),

    #[error(transparent)]
    Store(#[from] git::storage::Error),

    #[error(transparent)]
    Pool(#[from] deadpool::managed::PoolError<git::storage::Error>),

    #[error(transparent)]
    Task(executor::Cancelled),
}

impl From<executor::JoinError> for Error {
    fn from(e: executor::JoinError) -> Self {
        Self::Task(e.into_cancelled())
    }
}
