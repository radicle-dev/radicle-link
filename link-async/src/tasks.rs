// Copyright © 2021 The Radicle Foundation <hello@radicle.foundation>
// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use core::time::Duration;
use futures::{
    stream::{FuturesUnordered, StreamExt},
    Future,
    FutureExt,
    Stream,
};
use std::{
    marker::PhantomData,
    ops::{ControlFlow, Try},
    panic,
    pin::Pin,
    task::Poll,
};

/// Run tasks from a stream of tasks but terminate if the stream is idle for
/// `idle_timeout`. The idle timeout starts when there are no tasks running and
/// no new tasks to pull from the stream.
///
/// If the stream yields None this will drive all current tasks to completion
/// and then exit.
///
/// # Panics
///
/// Panics if any of the underlying tasks panics. In this case the remaining
/// tasks will not be driven to completion
pub fn run_until_idle<'a, T: 'a>(
    tasks: Pin<Box<dyn Stream<Item = crate::Task<T>> + Send + 'a>>,
    idle_timeout: Duration,
) -> impl futures::Future<Output = ()> + 'a {
    Tasks {
        tasks,
        idle_timeout: Some(idle_timeout),
        state: TasksState::Idle(Some(crate::sleep(idle_timeout).boxed())),
        _on_error: PhantomData::<Ignore<T>>,
    }
}

/// Run tasks from a stream of tasks which return `Result`s but terminate if the
/// stream is idle for `idle_timeout`. The idle timeout starts when there are no
/// tasks running and no new tasks to pull from the stream.
///
/// If one of the tasks returns an error then the future will resolve with the
/// error which was returned plus a stream of all the remaining tasks.
///
/// If the stream yields None this will drive all current tasks to completion
/// and then exit.
///
/// # Panics
///
/// Panics if any of the underlying tasks panics. In this case the remaining
/// tasks will not be driven to completion.
pub fn try_run_until_idle<'a, T: Try + 'a>(
    tasks: Pin<Box<dyn Stream<Item = crate::Task<T>> + Send + 'a>>,
    idle_timeout: Duration,
) -> impl futures::Future<
    Output = Result<(), (T::Residual, impl Stream<Item = Result<T, crate::JoinError>>)>,
> + 'a {
    Tasks {
        tasks,
        idle_timeout: Some(idle_timeout),
        state: TasksState::Idle(Some(crate::sleep(idle_timeout).boxed())),
        _on_error: PhantomData::<ReturnRemainingTasks<T>>,
    }
}

/// Run a stream of tasks until the stream returns None, at which point all
/// remaining tasks will be driven to completion
///
/// # Panics
///
/// Panics if any of the underlying tasks panics. In this case the remaining
/// tasks will not be driven to completion
pub fn run_forever<'a, T: 'a>(
    tasks: Pin<Box<dyn Stream<Item = crate::Task<T>> + Send + 'a>>,
) -> impl futures::Future<Output = ()> + 'a {
    Tasks {
        tasks,
        idle_timeout: None,
        state: TasksState::Idle(None),
        _on_error: PhantomData::<Ignore<T>>,
    }
}

/// Run a stream of tasks until the stream returns None, at which point all
/// remaining tasks will be driven to completion
///
/// If one of the tasks returns an error then the future will resolve with the
/// error which was returned plus a stream of all the remaining tasks.
///
/// # Panics
///
/// Panics if any of the underlying tasks panics. In this case the remaining
/// tasks will not be driven to completion
pub fn try_run_forever<'a, T: Try + 'a>(
    tasks: Pin<Box<dyn Stream<Item = crate::Task<T>> + Send + 'a>>,
) -> impl futures::Future<
    Output = Result<(), (T::Residual, impl Stream<Item = Result<T, crate::JoinError>>)>,
> + 'a {
    Tasks {
        tasks,
        idle_timeout: None,
        state: TasksState::Idle(None),
        _on_error: PhantomData::<ReturnRemainingTasks<T>>,
    }
}

/// Represents what to do when a task in a `Tasks` fails. This is useful because
/// it allows us to abstract over result types which have no concept of failure
/// (i.e. `T`) and result types which do have some notion of failure (i.e. `T:
/// Try`).
trait OnErrorPolicy<T> {
    /// The output type of the `Tasks` future
    type Output;
    /// The error type which can be extracted from `T`
    type Err;

    /// The output to return when the `Tasks` is complete
    fn done_output() -> Self::Output;

    /// Determine if the result of a task is an error which should cause the
    /// `Tasks` to resolve, returning the error value if it is an error
    fn extract_err(result: T) -> Option<Self::Err>;

    /// The output to return for the overall `Tasks` future if a task has failed
    fn error_output(
        err: Self::Err,
        remaining_tasks: FuturesUnordered<crate::Task<T>>,
    ) -> Self::Output;
}
/// Ignore failed tasks and continue to run the remaining tasks
struct Ignore<T> {
    _phantom: PhantomData<T>,
}

impl<T> Unpin for Ignore<T> {}

impl<T> OnErrorPolicy<T> for Ignore<T> {
    type Output = ();
    type Err = ();

    fn extract_err(_: T) -> Option<()> {
        None
    }

    fn done_output() -> Self::Output {}

    fn error_output(
        _err: Self::Err,
        _remaining_tasks: FuturesUnordered<crate::Task<T>>,
    ) -> Self::Output {
    }
}

/// Resolve the `Tasks` future with a (T::Residual, impl Stream<Item = Result<T,
/// crate::JoinError>>) if a task fails
struct ReturnRemainingTasks<T: Try> {
    _phantom: PhantomData<T>,
}

impl<T: Try> Unpin for ReturnRemainingTasks<T> {}

impl<T: Try> OnErrorPolicy<T> for ReturnRemainingTasks<T> {
    type Output = Result<(), (T::Residual, impl Stream<Item = Result<T, crate::JoinError>>)>;
    type Err = T::Residual;

    fn extract_err(result: T) -> Option<Self::Err> {
        match T::branch(result) {
            ControlFlow::Break(e) => Some(e),
            _ => None,
        }
    }

    fn done_output() -> Self::Output {
        Ok(())
    }

    fn error_output(
        err: Self::Err,
        remaining_tasks: FuturesUnordered<crate::Task<T>>,
    ) -> Self::Output {
        Err((err, remaining_tasks))
    }
}

/// A future which drives a stream of tasks
struct Tasks<'a, T, E: OnErrorPolicy<T>> {
    /// The tasks to drive
    tasks: Pin<Box<dyn Stream<Item = crate::Task<T>> + Send + 'a>>,
    /// How long to wait in the idle state before resolving the `Tasks`
    idle_timeout: Option<Duration>,
    /// The current state of the tasks
    state: TasksState<T>,
    /// The policy to apply if a task fails
    _on_error: PhantomData<E>,
}

enum TasksState<T> {
    /// There is some set of tasks currently executing
    Servicing {
        /// The tasks we are running
        ongoing_tasks: FuturesUnordered<crate::Task<T>>,
        /// If true then at some point the stream of tasks yielded `None` so we
        /// are just waiting for the ongoing tasks to finish
        finishing: bool,
    },
    /// There is no executing task. If this contains a Some(Sleep) then when the
    /// contained future wakes us up we will transition to `Dead`
    Idle(Option<Pin<Box<dyn Future<Output = ()> + Send>>>),
    /// There are no ongoing tasks and we are not looking for new ones. We
    /// always yield Poll::Ready(_on_error::done())
    Dead,
}

impl<'a, T, E: OnErrorPolicy<T> + Unpin> futures::Future for Tasks<'a, T, E> {
    type Output = E::Output;

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        if matches!(
            self.state,
            TasksState::Idle(..)
                | TasksState::Servicing {
                    finishing: false,
                    ..
                }
        ) {
            // If we're servicing (and not finishing) or idle then check for new tasks
            let mut new_tasks = Vec::new();
            let mut finish = false;
            while let Poll::Ready(maybe_task) = self.tasks.poll_next_unpin(cx) {
                match maybe_task {
                    Some(task) => {
                        new_tasks.push(task);
                    },
                    None => {
                        finish = true;
                        break;
                    },
                }
            }
            if let TasksState::Servicing {
                ongoing_tasks,
                finishing,
            } = &mut self.state
            {
                // if we're already in the `Servicing` state then add the tasks to the current
                // set of ongoing tasks. (Note that the pevious finishing: false match in
                // matches! above means that we won't add new tasks if we're
                // already finishing)
                ongoing_tasks.extend(new_tasks);
                if finish {
                    // If the tasks stream yielded `None` then flag the future as finishing
                    *finishing = true;
                }
            } else if !new_tasks.is_empty() {
                // otherwise transition to servicing
                self.state = TasksState::Servicing {
                    ongoing_tasks: new_tasks.into_iter().collect(),
                    finishing: finish,
                }
            }
        }
        match &mut self.state {
            TasksState::Servicing {
                ongoing_tasks,
                finishing,
            } => {
                while let Poll::Ready(Some(next_result)) = ongoing_tasks.poll_next_unpin(cx) {
                    match next_result {
                        Err(e) => {
                            if e.is_panic() {
                                panic::resume_unwind(e.into_panic());
                            }
                            tracing::warn!(err=?e, "task cancelled");
                        },
                        Ok(value) => {
                            if let Some(err) = E::extract_err(value) {
                                return Poll::Ready(E::error_output(
                                    err,
                                    std::mem::take(ongoing_tasks),
                                ));
                            }
                        },
                    }
                }
                if ongoing_tasks.is_empty() {
                    if *finishing {
                        self.state = TasksState::Dead;
                        Poll::Ready(E::done_output())
                    } else {
                        let mut sleep = self.idle_timeout.map(|t| crate::sleep(t).boxed());
                        #[allow(unused_must_use)]
                        if let Some(sleep) = &mut sleep {
                            // Schedule waker for the sleep
                            sleep.poll_unpin(cx);
                        }
                        self.state = TasksState::Idle(sleep);
                        Poll::Pending
                    }
                } else {
                    Poll::Pending
                }
            },
            TasksState::Idle(sleep) => {
                if let Some(sleep) = sleep {
                    match sleep.poll_unpin(cx) {
                        Poll::Ready(_) => {
                            self.state = TasksState::Dead;
                            Poll::Ready(E::done_output())
                        },
                        _ => Poll::Pending,
                    }
                } else {
                    Poll::Pending
                }
            },
            TasksState::Dead => Poll::Ready(E::done_output()),
        }
    }
}
