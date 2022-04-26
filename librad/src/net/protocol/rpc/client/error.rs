// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use thiserror::Error;

use crate::{
    git::storage,
    net::{
        protocol::{self, interrogation},
        quic,
        replication,
    },
    PeerId,
};

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Init {
    #[cfg(feature = "replication-v3")]
    #[error(transparent)]
    Replication(#[from] replication::error::Init),
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Interrogation {
    #[error("no response from {0}")]
    NoResponse(PeerId),

    #[error("error response: {0:?}")]
    ErrorResponse(interrogation::Error),

    #[error("invalid response")]
    InvalidResponse,

    #[error(transparent)]
    Rpc(#[from] Box<protocol::error::Rpc<quic::BidiStream>>),
}

impl From<protocol::error::Rpc<quic::BidiStream>> for Interrogation {
    fn from(e: protocol::error::Rpc<quic::BidiStream>) -> Self {
        Self::Rpc(Box::new(e))
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RequestPull {
    #[error("request-pull replication cancelled")]
    Cancelled,

    #[error(transparent)]
    Incoming(#[from] Incoming),

    #[error(transparent)]
    NoConnection(#[from] NoConnection),

    #[error(transparent)]
    Rpc(#[from] Box<protocol::error::Rpc<quic::BidiStream>>),
}

impl From<protocol::error::Rpc<quic::BidiStream>> for RequestPull {
    fn from(e: protocol::error::Rpc<quic::BidiStream>) -> Self {
        Self::Rpc(Box::new(e))
    }
}

#[derive(Debug, Error)]
pub enum Replicate {
    #[error(transparent)]
    NoConnection(#[from] NoConnection),

    #[error("failed to borrow storage from pool")]
    Pool(#[from] storage::PoolError),

    #[error(transparent)]
    Replicate(#[from] replication::error::Replicate),
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Storage {
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

#[derive(Debug, Error)]
#[error("unable to obtain connection to {0}")]
pub struct NoConnection(pub PeerId);

#[derive(Debug, Error)]
pub enum Incoming {
    #[error(transparent)]
    Quic(#[from] quic::error::Error),
    #[error("expected bidirectional connection, but found a unidirectional connection")]
    Uni,
    #[error("connection lost")]
    ConnectionLost,
}
