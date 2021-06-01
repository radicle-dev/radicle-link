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

use futures::{
    channel::mpsc::{self, UnboundedReceiver, UnboundedSender},
    stream::{FusedStream as _, FuturesUnordered, Stream as _},
    task::AtomicWaker,
    FutureExt as _,
};
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
    detach: UnboundedSender<DetachedTask>,

    pid1: Arc<AtomicWaker>,

    spawned: Arc<AtomicUsize>,
    blocking: Arc<AtomicUsize>,
}

impl Spawner {
    /// Create a new [`Spawner`] with a scope label.
    ///
    /// The scope label is for informational purposes (logging, tracing) only.
    pub fn new<S: AsRef<str>>(scope: S) -> Self {
        let rt = tokio::runtime::Handle::current();
        let (tx_submit, rx_submit) = mpsc::unbounded();
        let waker = Arc::new(AtomicWaker::new());

        rt.spawn(
            Pid1 {
                submit: rx_submit,
                running: FuturesUnordered::new(),
                waker: Arc::clone(&waker),
            }
            .instrument(tracing::info_span!("pid1", scope = %scope.as_ref())),
        );

        Self {
            scope: scope.as_ref().to_owned(),
            inner: rt,
            detach: tx_submit,
            pid1: waker,
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
            detach: self.detach.clone(),
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
            detach: self.detach.clone(),
            task: self.inner.spawn_blocking(move || {
                counter.fetch_add(1, Relaxed);
                let _guard = span.enter();
                let res = f();
                counter.fetch_sub(1, Relaxed);
                res
            }),
        }
    }

    pub fn stats(&self) -> Stats {
        Stats {
            scope: &self.scope,
            spawned: self.spawned.load(Relaxed),
            blocking: self.blocking.load(Relaxed),
        }
    }
}

impl Drop for Spawner {
    fn drop(&mut self) {
        tracing::debug!("{} spawner drop, awakening the beast", self.scope);
        self.pid1.wake()
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
    detach: UnboundedSender<DetachedTask>,
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
    /// A detached task will continue to run until the [`Spawner`] through which
    /// it was created is dropped. When this happens, the task is aborted as
    /// if [`JoinHandle::abort`] was called.
    pub fn detach(self) {
        if let Err(e) = self.detach.unbounded_send(DetachedTask(self.task)) {
            tracing::warn!("detach queue closed, task will be cancelled");
            drop(e.into_inner())
        }
    }
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

struct DetachedTask(tokio::task::JoinHandle<()>);

impl Drop for DetachedTask {
    fn drop(&mut self) {
        self.0.abort()
    }
}

impl Future for DetachedTask {
    type Output = Result<(), JoinError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        self.0.poll_unpin(cx).map(|t| t.map_err(JoinError::from))
    }
}

/// Keeps track of [`DetachedTask`]s spawned through a particular [`Spawner`],
/// dropping their handles once their futures resolve. When the [`Spawner`] is
/// dropped, its [`Pid1`] is aborted, which in turn will drop all in-flight
/// tasks.
struct Pid1 {
    submit: UnboundedReceiver<DetachedTask>,
    running: FuturesUnordered<DetachedTask>,
    waker: Arc<AtomicWaker>,
}

impl Pid1 {
    fn poll_submitted(&mut self, cx: &mut Context) {
        while let Poll::Ready(Some(task)) = Pin::new(&mut self.submit).poll_next(cx) {
            self.running.push(task)
        }
    }

    fn poll_running(&mut self, cx: &mut Context) {
        while let Poll::Ready(Some(result)) = Pin::new(&mut self.running).poll_next(cx) {
            if let Err(JoinError::Panicked(panik)) = result {
                tracing::error!(
                    "detached task panicked: {:?}",
                    panik.downcast_ref::<String>()
                )
            }
        }
    }
}

impl Future for Pid1 {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        tracing::debug!("pid1 awoken");
        self.waker.register(cx.waker());
        self.poll_submitted(cx);
        self.poll_running(cx);

        if self.submit.is_terminated() {
            tracing::debug!("pid1 done");
            Poll::Ready(())
        } else {
            tracing::debug!("pid1 pending");
            Poll::Pending
        }
    }
}
