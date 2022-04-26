// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! The purpose of this module is to provide a way to block on futures which
//! does not block the current tokio runtime in
//! `link_crypto::Signer::sign_blocking`. In order to do this we start a
//! static runtime and send tasks to it via a `std::sync::mpsc::Channel`. Tasks
//! are any future which is `Send + 'static`.
use std::{pin::Pin, sync::Arc};

use once_cell::sync::Lazy;

static RUNTIME: Lazy<Runtime> = Lazy::new(Runtime::new);

/// Submit a task to the static runtime and wait for its output. The task is run
/// within the context of a separate multi threaded tokio runtime, which means
/// that spawned sub-tasks will execute correctly. If the thread waiting for
/// this task panics then the running task will be cancelled at the next await
/// point.
pub(crate) fn block_on<F>(future: F) -> F::Output
where
    F: futures::Future + Send + 'static,
    F::Output: Send + 'static,
{
    let job = RUNTIME.spawn(future);
    job.wait().unwrap()
}

pub(crate) struct Runtime {
    requests: tokio::sync::mpsc::UnboundedSender<Task>,
}

impl Runtime {
    pub fn new() -> Self {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Task>();

        std::thread::spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("unable to create tokio runtime");
            let rt_handle = runtime.handle();
            runtime.block_on(async move {
                // Dropping the tokio runtime only waits for tasks to yield not to complete
                //
                // We therefore use a RwLock to wait for tasks to complete
                let join = Arc::new(tokio::sync::RwLock::new(()));

                while let Some(task) = rx.recv().await {
                    let join = Arc::clone(&join);
                    let handle = join.read_owned().await;

                    rt_handle.spawn(async move {
                        task.run().await;
                        std::mem::drop(handle);
                    });
                }
                join.write().await;
            });
        });

        Runtime { requests: tx }
    }

    pub fn spawn<T>(&self, task: T) -> Job<T::Output>
    where
        T: futures::Future + Send + 'static,
        T::Output: Send + 'static,
    {
        let (tx, rx) = std::sync::mpsc::channel::<T::Output>();

        let fut = Box::pin(async move {
            let task_output = task.await;
            if tx.send(task_output).is_err() {
                tracing::warn!("spawned task output ignored, receiver dropped");
            }
        });

        let task = Task { fut };
        self.requests.send(task).ok();

        Job { rx }
    }
}

struct Task {
    fut: Pin<Box<dyn futures::Future<Output = ()> + Send>>,
}

impl Task {
    async fn run(self) {
        self.fut.await
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Recv(#[from] std::sync::mpsc::RecvError),
}

pub(crate) struct Job<T> {
    rx: std::sync::mpsc::Receiver<T>,
}

impl<T> Job<T> {
    pub(crate) fn wait(self) -> Result<T, Error> {
        self.rx.recv().map_err(Error::from)
    }
}
