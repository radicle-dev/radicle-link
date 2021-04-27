// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::panic;

use thiserror::Error;
use tokio::task::JoinError;

use crate::git::{self, replication, storage::fetcher, tracking};

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

    #[error("malformed reference")]
    Reference,

    #[error(transparent)]
    Store(#[from] git::storage::Error),

    #[error(transparent)]
    Pool(#[from] deadpool::managed::PoolError<git::storage::Error>),

    #[error("spawned task was cancelled")]
    Cancelled,
}

impl From<JoinError> for Error {
    fn from(e: JoinError) -> Self {
        if e.is_cancelled() {
            Self::Cancelled
        } else if e.is_panic() {
            panic::resume_unwind(e.into_panic())
        } else {
            panic!("unexpected task error: {:?}", e)
        }
    }
}
