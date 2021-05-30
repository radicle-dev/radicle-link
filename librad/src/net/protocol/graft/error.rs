// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::fmt::Debug;

use futures::channel::{mpsc, oneshot};
use thiserror::Error;

#[derive(Debug, Error)]
#[error("input didn't pass policy")]
pub struct Policy;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Queue<C: Debug> {
    #[error("task queue at capacity")]
    HighWaterMark(C),
    #[error("task scheduler went away")]
    SchedulerLost(C),
}

impl<C: Debug, R> From<mpsc::TrySendError<(C, R)>> for Queue<C> {
    fn from(e: mpsc::TrySendError<(C, R)>) -> Self {
        if e.is_full() {
            Self::HighWaterMark(e.into_inner().0)
        } else if e.is_disconnected() {
            Self::SchedulerLost(e.into_inner().0)
        } else {
            unreachable!("unexpected `TrySendError` variant: {:?}", e)
        }
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Trigger<C: Debug + 'static> {
    #[error(transparent)]
    Policy(#[from] Policy),
    #[error(transparent)]
    Queue(#[from] Queue<C>),
}

#[derive(Debug, Error)]
pub enum Scheduler {
    #[error("task cancelled, scheduler went away")]
    Cancelled,
}

impl From<oneshot::Canceled> for Scheduler {
    fn from(_: oneshot::Canceled) -> Self {
        Self::Cancelled
    }
}
