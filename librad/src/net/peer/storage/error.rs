// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use thiserror::Error;

use crate::{
    git::{self, storage, tracking},
    net::replication,
    PeerId,
};

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("already have {0}")]
    KnownObject(git2::Oid),

    #[error("too many fetches from {remote_peer}")]
    RateLimited { remote_peer: PeerId, urn: git::Urn },

    #[error("no connection to {remote_peer}")]
    NoConnection { remote_peer: PeerId },

    #[error(transparent)]
    Tracking(#[from] tracking::Error),

    #[error(transparent)]
    Replication(#[from] replication::error::Replicate),

    #[error(transparent)]
    Store(#[from] storage::Error),

    #[error(transparent)]
    Pool(#[from] storage::PoolError),
}
