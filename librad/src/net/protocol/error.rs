// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::fmt::Debug;

use thiserror::Error;

use super::{graft, interrogation, io};
use crate::{git::storage::pool::PoolError, net::quic, PeerId};
use tokio::sync::oneshot;

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

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Graft {
    #[error("network stack not available")]
    Unavailable,

    #[error("unable to obtain a connection to {0}")]
    NoConnection(PeerId),

    #[error("task queue at capacity")]
    Full,

    #[error("task scheduler not running")]
    Stopped,

    #[error(transparent)]
    Prepare(#[from] Box<io::graft::error::Prepare>),

    #[error(transparent)]
    Step(#[from] io::graft::error::Step),
}

impl<C: Debug> From<graft::error::Queue<C>> for Graft {
    fn from(e: graft::error::Queue<C>) -> Self {
        use graft::error::Queue::*;

        match e {
            HighWaterMark(_) => Self::Full,
            SchedulerLost(_) => Self::Stopped,
        }
    }
}

impl From<io::graft::error::Prepare> for Graft {
    fn from(e: io::graft::error::Prepare) -> Self {
        Self::from(Box::new(e))
    }
}

impl From<graft::error::Scheduler> for Graft {
    fn from(graft::error::Scheduler::Cancelled: graft::error::Scheduler) -> Self {
        Self::Stopped
    }
}

impl From<oneshot::error::RecvError> for Graft {
    fn from(_: oneshot::error::RecvError) -> Self {
        Self::Unavailable
    }
}
