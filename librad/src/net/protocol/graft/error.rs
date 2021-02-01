// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use thiserror::Error;

use crate::git::{identities, replication, storage::pool::PoolError};

#[derive(Debug, Error)]
pub enum Ask {
    #[error("invalid bloom filter: {0}")]
    Bloom(&'static str),

    #[error(transparent)]
    Git(#[from] identities::Error),
}

#[derive(Debug, Error)]
pub enum Offer {
    #[error("unable to borrow pooled storage")]
    Pool(#[from] PoolError),

    #[error(transparent)]
    Replication(#[from] replication::Error),

    #[error("handling task was cancelled")]
    Cancelled,
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum State {
    #[error(transparent)]
    Git(#[from] identities::Error),
}
