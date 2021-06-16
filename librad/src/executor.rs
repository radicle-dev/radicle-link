// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    any::Any,
    future::Future,
    panic,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering::Relaxed},
        Arc,
    },
    task::{Context, Poll},
};

use futures::FutureExt as _;
use thiserror::Error;
use tracing::Instrument as _;

/// Wrapper around an async runtime, which:
///
/// * Approximates a task scope
/// * Commits to a semantics where mainstream runtimes disagree
/// * Adds instrumentation to spawned tasks
/// * May allow compile-time selection of the runtime implementation at some
///   point (currently only `tokio` is supported)
///
/// When a [`Spawner`] is dropped, all tasks spawned through it are cancelled.
/// Note, however, that mainstream runtimes **do not** guarantee that those
/// tasks are cancelled _immediately_.
pub struct Spawner {
    scope: String,
    inner: tokio::runtime::Handle,

    spawned: Arc<AtomicUsize>,
    blocking: Arc<AtomicUsize>,
}

impl Spawner {
    /// Create a new [`Spawner`] with a scope label.
    ///
    /// The scope label is for informational purposes (logging, tracing) only.
    pub fn new<S: AsRef<str>>(scope: S) -> Self {
        let rt = tokio::runtime::Handle::current();
        Self {
            scope: scope.as_ref().to_owned(),
            inner: rt,
            spawned: Arc::new(AtomicUsize::new(0)),
            blocking: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn spawn<T>(&self, task: T) -> JoinHandle<T::Output>
    where
        T: Future + Send + 'static,
        T::Output: Send + 'static,
    {
        let counter = Arc::clone(&self.spawned);
        JoinHandle {
            task: self.inner.spawn(
                async move {
                    counter.fetch_add(1, Relaxed);
                    let res = task.await;
                    counter.fetch_sub(1, Relaxed);
                    res
                }
                .in_current_span(),
            ),
        }
    }

    pub fn spawn_blocking<F, T>(&self, f: F) -> JoinHandle<T>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let span = tracing::Span::current();
        let counter = Arc::clone(&self.blocking);
        JoinHandle {
            task: self.inner.spawn_blocking(move || {
                counter.fetch_add(1, Relaxed);
                let _guard = span.enter();
                let res = f();
                counter.fetch_sub(1, Relaxed);
                res
            }),
        }
    }

    pub fn block_in_place<F, T>(&self, f: F) -> T
    where
        F: FnOnce() -> T,
    {
        let span = tracing::Span::current();
        let _tracing = span.enter();
        let _rt = self.inner.enter();
        self.blocking.fetch_add(1, Relaxed);
        let res = tokio::task::block_in_place(f);
        self.blocking.fetch_sub(1, Relaxed);
        res
    }

    pub fn stats(&self) -> Stats {
        Stats {
            scope: &self.scope,
            spawned: self.spawned.load(Relaxed),
            blocking: self.blocking.load(Relaxed),
        }
    }
}

/// Snapshot of the state of a [`Spawner`].
pub struct Stats<'a> {
    /// Scope label of the [`Spawner`].
    pub scope: &'a str,
    /// Number of tasks spawned using [`Spawner::spawn`] whose futures have not
    /// resoved yet. Includes detached tasks.
    pub spawned: usize,
    /// Number of tasks spawned using [`Spawner::spawn_blocking`] whose futures
    /// have not resolved yet. Includes detached tasks.
    pub blocking: usize,
}

/// A handle to a task spawned via [`Spawner::spawn`] or
/// [`Spawner::spawn_blocking`].
///
/// Dropping a [`JoinHandle`] will abort the task, ie. `spawn(task);` is a
/// no-op. To continue running the task without polling the [`JoinHandle`]
/// future, [`JoinHandle::detach`] can be used.
///
/// This is similar to `async-std`, but very unlike `tokio`.
#[must_use = "spawned tasks must be awaited"]
pub struct JoinHandle<T> {
    task: tokio::task::JoinHandle<T>,
}

impl<T> JoinHandle<T> {
    /// Abort the task corresponding to this [`JoinHandle`].
    ///
    /// The task will be dropped immediately and not polled again -- _unless_ it
    /// is currently being polled, in which case the task can be considered
    /// cancelled only after poll returns. Iow it is not guaranteed that the
    /// task is cancelled when this function returns.
    pub fn abort(&self) {
        self.task.abort()
    }
}

impl JoinHandle<()> {
    /// If the underlying task does not yield any output, it can be "detached".
    ///
    /// A detached task will continue to run until it terminates on it's own.
    /// Dropping the runtime will typically block on outstanding tasks.
    pub fn detach(self) {}
}

impl<T> Future for JoinHandle<T> {
    type Output = Result<T, JoinError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        self.task.poll_unpin(cx).map(|t| t.map_err(JoinError::from))
    }
}

#[derive(Debug, Error)]
pub enum JoinError {
    #[error("task cancelled")]
    Cancelled,
    #[error("task panicked")]
    Panicked(Box<dyn Any + Send + 'static>),
}

impl JoinError {
    /// Test if [`JoinError::into_cancelled`] would panic.
    pub fn is_panic(&self) -> bool {
        match self {
            Self::Cancelled => false,
            Self::Panicked(_) => true,
        }
    }

    /// If `self` is [`JoinError::Cancelled`], returns [`Cancelled`], otherwise
    /// resumes the panic contained in [`JoinError::Panicked`].
    pub fn into_cancelled(self) -> Cancelled {
        match self {
            Self::Cancelled => Cancelled,
            Self::Panicked(panik) => panic::resume_unwind(panik),
        }
    }
}

impl From<tokio::task::JoinError> for JoinError {
    fn from(e: tokio::task::JoinError) -> Self {
        if e.is_cancelled() {
            Self::Cancelled
        } else if e.is_panic() {
            Self::Panicked(e.into_panic())
        } else {
            unreachable!("unexpected join error: {:?}", e)
        }
    }
}

#[derive(Debug, Error)]
#[error("spawned task cancelled")]
pub struct Cancelled;
