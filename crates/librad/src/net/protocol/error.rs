// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::fmt::Debug;

use thiserror::Error;

use super::interrogation;
use crate::{git::storage::pool::PoolError, net::quic, PeerId};

mod internal;
pub(super) use internal::*;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Bootstrap {
    #[error(transparent)]
    Pool(#[from] PoolError),

    #[error(transparent)]
    Quic(#[from] quic::Error),
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Interrogation {
    #[error("unable to obtain a connection to {0}")]
    NoConnection(PeerId),

    #[error("no response from {0}")]
    NoResponse(PeerId),

    #[error("error response: {0:?}")]
    ErrorResponse(interrogation::Error),

    #[error("invalid response")]
    InvalidResponse,

    #[error("network stack not available")]
    Unavailable,

    #[error(transparent)]
    Rpc(#[from] Box<internal::Rpc<quic::BidiStream>>),
}

impl From<internal::Rpc<quic::BidiStream>> for Interrogation {
    fn from(e: internal::Rpc<quic::BidiStream>) -> Self {
        Self::Rpc(Box::new(e))
    }
}
