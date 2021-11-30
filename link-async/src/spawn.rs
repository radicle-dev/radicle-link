// Copyright © 2021 The Radicle Foundation <hello@radicle.foundation>
// Copyright © 2021 The Radicle Link Contributors
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

use futures_util::FutureExt as _;
use thiserror::Error;
use tracing::Instrument as _;

/// Wrapper around an async runtime.
pub struct Spawner {
    inner: tokio::runtime::Handle,
    stats: StatsMut,
}

impl Spawner {
    /// Try to create a [`Spawner`] from the ambient async context.
    ///
    /// Returns `None` if the current thread does not have access to an async
    /// context. Runtimes typically expose an `.enter()` function in some
    /// form, which can be used to propagate the context in this case.
    pub fn from_current() -> Option<Self> {
        tokio::runtime::Handle::try_current().map(Self::tokio).ok()
    }

    /// Create a [`Spawner`] from a [`tokio::runtime::Handle`].
    pub fn tokio(inner: tokio::runtime::Handle) -> Self {
        Self {
            inner,
            stats: StatsMut {
                spawned: Arc::new(AtomicUsize::new(0)),
                blocking: Arc::new(AtomicUsize::new(0)),
            },
        }
    }

    /// Spawn an asynchronous task, returning a handle to it.
    ///
    /// Spawning a task enables it to run concurrently and in parallel to other
    /// tasks. The task may run on the current thread, or it may be sent to
    /// a different thread by the runtime to be executed.
    ///
    /// The returned [`Task`] future can be `.await`ed in order to retrieve the
    /// task's output. Note, however, that the task will be executed
    /// regardless of whether the [`Task`] handle is `.await`ed.
    ///
    /// The `spawned` counter of [`Stats`] will be incremented once the task is
    /// scheduled for execution, and decremented when its future completes.
    ///
    /// The task is run in the [`tracing::Span`] context active at the call site
    /// of [`spawn()`][`Spawner::spawn`].
    ///
    /// # Cancellation
    ///
    /// Dropping the [`Task`] will abort the task, ie. it will be deallocated at
    /// the next possible time, regardless of whether the task ran to
    /// completion already. To continue running the task in the background,
    /// [`Task::detach`] can be called. In this case, the output of the task
    /// (if any) can no longer be retrieved, and the task will continue to
    /// run until it either completes, or the runtime shuts down.
    ///
    /// Keep in mind that there is no guarantee that the task is run to
    /// completion -- the runtime may decide to deallocate it when it shuts
    /// down. It is guaranteed, however, that the task is cancelled when the
    /// [`Task`] is cancelled.
    pub fn spawn<T>(&self, task: T) -> Task<T::Output>
    where
        T: Future + Send + 'static,
        T::Output: Send + 'static,
    {
        let counter = Arc::clone(&self.stats.spawned);
        self.inner
            .spawn(
                async move {
                    counter.fetch_add(1, Relaxed);
                    let res = task.await;
                    counter.fetch_sub(1, Relaxed);
                    res
                }
                .in_current_span(),
            )
            .into()
    }

    /// Run a blocking function in an async context.
    ///
    /// The function is run on a separate thread pool, so as to not block the
    /// async runtime's threads. The current async context is made available to
    /// the thread executing the task, so it can be re-entered.
    ///
    /// The `blocking` counter of [`Stats`] will be incremented once the task is
    /// scheduled for execution, and decremented when the function completes.
    ///
    /// The task is run in the [`tracing::Span`] context active at the call site
    /// of [`blocking()`][`Spawner::blocking`].
    ///
    /// # Panics
    ///
    /// If the blocking function panics, the `.await`ing future will also panic.
    ///
    /// _NOTE: Due to limitations in the underlying machinery, the panic payload
    /// will always be 'task has failed'. The default panic hook should print
    /// the original panic message, and be able to display a backtrace,
    /// however._
    ///
    /// # Cancellation
    ///
    /// Blocking tasks can _not_ be cancelled, even if the future awaiting their
    /// output is dropped. This can lead to surprising behaviour, eg. when the
    /// blocking code is accessing resources which are destroyed when the
    /// program exits. It is the programmer's responsibility to ensure
    /// "graceful shutdown" by driving outstanding futures which may
    /// `.await` blocking tasks to completion.
    pub async fn blocking<F, T>(&self, f: F) -> T
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let rt = self.inner.clone();
        let span = tracing::Span::current();
        let counter = Arc::clone(&self.stats.blocking);
        blocking::unblock(move || {
            counter.fetch_add(1, Relaxed);
            let _span = span.enter();
            let _rt = rt.enter();
            let res = f();
            counter.fetch_sub(1, Relaxed);
            res
        })
        .await
    }

    /// Obtain a snapshot of some stats about this [`Spawner`].
    pub fn stats(&self) -> Stats {
        self.stats.snapshot()
    }
}

struct StatsMut {
    spawned: Arc<AtomicUsize>,
    blocking: Arc<AtomicUsize>,
}

impl StatsMut {
    fn snapshot(&self) -> Stats {
        Stats {
            spawned: self.spawned.load(Relaxed),
            blocking: self.blocking.load(Relaxed),
        }
    }
}

/// Snapshot of the state of a [`Spawner`].
pub struct Stats {
    /// Number of tasks spawned using [`Spawner::spawn`] whose futures have not
    /// resoved yet. Includes detached tasks.
    pub spawned: usize,
    /// Number of tasks spawned using [`Spawner::blocking`] whose futures
    /// have not resolved yet. Includes detached tasks.
    pub blocking: usize,
}

/// A handle to a task spawned via [`Spawner::spawn`].
///
/// Dropping a [`Task`] will abort the task, ie. `spawn(task);` is a
/// no-op. To continue running the task without polling the [`Task`]
/// future, [`Task::detach`] can be used.
///
/// _NOTE: This is similar to `async-std`, but very unlike `tokio`._
#[must_use = "spawned tasks must be awaited"]
pub struct Task<T> {
    task: tokio::task::JoinHandle<T>,
    abort_on_drop: bool,
}

impl<T> Task<T> {
    /// Abort the task corresponding to this [`Task`].
    ///
    /// The task will be dropped immediately and not polled again -- _unless_ it
    /// is currently being polled, in which case the task can be considered
    /// cancelled only after poll returns. Iow it is not guaranteed that the
    /// task is cancelled when this function returns.
    pub fn abort(&self) {
        self.task.abort()
    }

    /// Continue running the [`Task`] in the background.
    pub fn detach(mut self) {
        self.abort_on_drop = false;
    }
}

impl<T> From<tokio::task::JoinHandle<T>> for Task<T> {
    fn from(task: tokio::task::JoinHandle<T>) -> Self {
        Self {
            task,
            abort_on_drop: true,
        }
    }
}

impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        if self.abort_on_drop {
            self.abort()
        }
    }
}

impl<T> Future for Task<T> {
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

    /// Consumes the join error, returning the object with which the task
    /// panicked.
    ///
    /// # Panics
    /// into_panic() panics if the Error does not represent the underlying task
    /// terminating with a panic. Use is_panic to check the error reason.
    // Note: This documentation is copied verbatim from tokio::task::JoinError
    pub fn into_panic(self) -> Box<dyn Any + Send + 'static> {
        match self {
            Self::Cancelled => panic!("Task was cancelled, not panicked"),
            Self::Panicked(payload) => payload,
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
