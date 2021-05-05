// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use thiserror::Error;

use crate::{
    git::{
        identities,
        refs,
        replication,
        storage::{self, fetcher},
        Urn,
    },
    net::{protocol, quic},
    PeerId,
};

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Rere {
    #[error("precomputed signed refs for {0} not found")]
    MissingSignedRefs(Urn),

    #[error(transparent)]
    Replicate(#[from] replication::Error),

    #[error(transparent)]
    Refs(#[from] refs::stored::Error),

    #[error("unable to obtain fetcher")]
    Fetcher(#[from] fetcher::error::Retrying<git2::Error>),

    #[error(transparent)]
    Storage(#[from] storage::Error),

    #[error(transparent)]
    Pool(#[from] storage::PoolError),
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Prepare {
    #[error("no response from {from}")]
    NoResponse { from: PeerId },

    #[error("invalid response from {from}")]
    InvalidResponse { from: PeerId },

    #[error(transparent)]
    Identities(#[from] Box<identities::error::Error>),

    #[error(transparent)]
    Rpc(#[from] Box<protocol::error::Rpc<quic::BidiStream>>),

    #[error(transparent)]
    Pool(#[from] storage::PoolError),
}

impl From<identities::error::Error> for Prepare {
    fn from(e: identities::error::Error) -> Self {
        Self::Identities(Box::new(e))
    }
}

impl From<protocol::error::Rpc<quic::BidiStream>> for Prepare {
    fn from(e: protocol::error::Rpc<quic::BidiStream>) -> Self {
        Self::from(Box::new(e))
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Step {
    #[error(transparent)]
    Replication(#[from] replication::Error),

    #[error("unable to acquire fetcher")]
    Fetcher(#[from] fetcher::error::Retrying<git2::Error>),
}
