// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{pin::Pin, sync::Arc, task::Poll, time::Duration};

use futures::{future::FutureExt, Stream};

use link_async::{Spawner, Task};

/// Generates a stream of task spawned on `spawner` using `task_factory`.
/// After `delay_after` tasks have been yielded the stream will wait for `delay`
/// until yielding the next task
pub fn delayed_task_stream<T, F>(
    spawner: Arc<Spawner>,
    delay: Duration,
    delay_after: usize,
    task_factory: F,
) -> Pin<Box<impl Stream<Item = Task<T>>>>
where
    T: Unpin,
    F: Fn(Arc<Spawner>) -> Task<T> + Unpin,
{
    Box::pin(DelayedStream::new(
        spawner,
        task_factory,
        delay,
        delay_after,
    ))
}

/// A stream of tasks which inserts a delay of `delay` after `delay_after`
/// elements have been yielded
struct DelayedStream<T, F: Fn(Arc<Spawner>) -> Task<T>> {
    spawner: Arc<Spawner>,
    task_factory: F,
    delay: Duration,
    delay_after: usize,
    state: DelayedState,
}

enum DelayedState {
    Yielding { current_elem: usize },
    Delaying(Pin<Box<tokio::time::Sleep>>),
}

impl<T: Unpin, F: Fn(Arc<Spawner>) -> Task<T> + Unpin> DelayedStream<T, F> {
    fn new(
        spawner: Arc<Spawner>,
        task_factory: F,
        delay: Duration,
        delay_after: usize,
    ) -> DelayedStream<T, F> {
        DelayedStream {
            spawner,
            task_factory,
            delay,
            delay_after,
            state: DelayedState::Yielding { current_elem: 0 },
        }
    }
}

impl<T: Unpin, F: Fn(Arc<Spawner>) -> Task<T> + Unpin> futures::Stream for DelayedStream<T, F> {
    type Item = Task<T>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let delay_after = self.delay_after;
        match &mut self.state {
            DelayedState::Yielding { current_elem } => {
                if *current_elem == delay_after {
                    let mut sleep = Box::pin(tokio::time::sleep(self.delay));
                    #[allow(unused_must_use)]
                    {
                        sleep.poll_unpin(cx);
                    }
                    self.state = DelayedState::Delaying(sleep);
                    Poll::Pending
                } else {
                    *current_elem += 1;
                    Poll::Ready(Some((self.task_factory)(self.spawner.clone())))
                }
            },
            DelayedState::Delaying(sleep) => match sleep.poll_unpin(cx) {
                Poll::Ready(_) => {
                    self.state = DelayedState::Yielding {
                        current_elem: self.delay_after + 1,
                    };
                    Poll::Ready(Some((self.task_factory)(self.spawner.clone())))
                },
                Poll::Pending => Poll::Pending,
            },
        }
    }
}
