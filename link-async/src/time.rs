// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures_util::{FutureExt as _, Stream};
use thiserror::Error;

#[derive(Debug, Error)]
#[error("timeout elapsed")]
pub struct Elapsed;

/// Requires a [`Future`] to complete before the specified duration elapsed.
///
/// If the future completes before the duration elapsed, then the completed
/// value is returned. Otherwise, an error is returned and the future is
/// dropped.
///
/// # Cancellation
///
/// No special measures are taken to cancel the supplied [`Future`] -- it is
/// simply dropped if either the timeout elapsed or the future returned by
/// calling [`timeout`] is dropped. That is, it is the caller's responsibility
/// to ensure cancellation-safety of the provided future.
///
/// It is not currently possible to cancel the timeout, and get back the future
/// for further scheduling.
pub async fn timeout<F, T>(after: Duration, f: F) -> Result<T, Elapsed>
where
    F: Future<Output = T>,
{
    tokio::time::timeout(after, f).await.map_err(|_| Elapsed)
}

/// Wait until `duration` has elapsed.
///
/// No work is performed while awaiting on the sleep future to complete.
/// Awaiting the future can be expected to not return for _at least_ `duration`,
/// but not precisely at the point it elapsed.
///
/// # Cancellation
///
/// A sleep can be cancelled by dropping its future.
pub async fn sleep(duration: Duration) {
    tokio::time::sleep(duration).await
}

/// The [`Stream`] created by [`interval`].
pub struct Interval {
    snooze: Pin<Box<tokio::time::Sleep>>,
    period: Duration,
    jitter: Duration,
}

/// Create a [`Stream`] which yields every `period`.
///
/// Whenever `period` elapses, a new period is calculated by either adding or
/// subtracting a duration between zero and `jitter` to the configured `period`
/// duration. The granularity for jitter is one second.
///
/// # Cancellation
///
/// An interval can be cancelled by dropping it.
pub fn interval(period: Duration, jitter: Duration) -> Interval {
    Interval {
        snooze: Box::pin(tokio::time::sleep(period)),
        period,
        jitter,
    }
}

impl Stream for Interval {
    type Item = ();

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        use rand::Rng as _;

        self.snooze.poll_unpin(cx).map(|()| {
            let mut rng = rand::thread_rng();
            let jitter = Duration::from_secs(rng.gen_range(0..=self.jitter.as_secs()));
            let delay = if rng.gen() {
                self.period.saturating_add(jitter)
            } else {
                self.period.saturating_sub(jitter)
            };
            let deadline = tokio::time::Instant::now() + delay;
            self.snooze.as_mut().reset(deadline);

            Some(())
        })
    }
}
