// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{pin::Pin, sync::Arc, time::Duration};

use futures::{future::lazy, Stream, StreamExt};

use link_async::{tasks, Spawner, Task};

use crate::link_async::delayed_task_stream;

type TaskStream<T> = Pin<Box<dyn Stream<Item = Task<T>> + Send>>;

fn panicking_stream<'a, T: 'static + Send>(
    spawner: Spawner,
    first_elem: T,
) -> Pin<Box<dyn Stream<Item = Task<T>> + Send + 'a>> {
    let task1 = spawner.spawn(lazy(move |_| first_elem));
    let task2 = spawner.spawn(lazy(|_| panic!("die!")));
    futures::stream::iter(vec![task1, task2]).boxed()
}

#[tokio::test]
#[should_panic]
async fn run_forever_panics_if_subtask_panics() {
    let spawner = Spawner::from_current().unwrap();
    let stream = panicking_stream(spawner, 1);
    tasks::run_forever(stream).await;
}

#[tokio::test]
#[should_panic]
async fn try_run_forever_panics_if_subtask_panics() {
    let spawner = Spawner::from_current().unwrap();
    let stream: TaskStream<Result<i32, ()>> = panicking_stream(spawner, Ok(1));
    let _ = tasks::try_run_forever(stream).await;
}

#[tokio::test]
#[should_panic]
async fn run_until_idle_panics_if_subtask_panics() {
    let spawner = Spawner::from_current().unwrap();
    let stream = panicking_stream(spawner, 1);
    tasks::run_until_idle(stream, Duration::from_secs(1)).await;
}

#[tokio::test]
#[should_panic]
async fn try_run_until_idle_panics_if_subtask_panics() {
    let spawner = Spawner::from_current().unwrap();
    let stream: TaskStream<Result<i32, ()>> = panicking_stream(spawner, Ok(1));
    let _ = tasks::try_run_until_idle(stream, Duration::from_secs(1)).await;
}

fn finite_stream<'a, T: 'static + Send>(
    elems: Vec<T>,
) -> Pin<Box<dyn Stream<Item = Task<T>> + Send + 'a>> {
    let spawner = Spawner::from_current().unwrap();
    let tasks = elems
        .into_iter()
        .map(move |elem| spawner.spawn(lazy(|_| elem)));
    futures::stream::iter(tasks).boxed()
}

#[tokio::test]
async fn run_forever_completes_when_stream_completes() {
    let stream = finite_stream(vec![1, 2, 3]);
    tasks::run_forever(stream).await;
}

#[tokio::test]
async fn run_until_idle_completes_when_stream_completes() {
    let stream = finite_stream(vec![1, 2, 3]);
    tasks::run_until_idle(stream, Duration::from_secs(1)).await;
}

#[tokio::test]
async fn run_until_idle_completes_when_idle() {
    let spawner = Arc::new(Spawner::from_current().unwrap());
    let stream = delayed_task_stream(spawner, Duration::from_secs(2), 10, |spawner| {
        spawner.spawn(async {
            link_async::sleep(Duration::from_millis(50)).await;
            1
        })
    })
    .boxed();
    tasks::run_until_idle(stream, Duration::from_secs(1)).await
}

#[tokio::test]
async fn run_until_idle_waits_for_running_tasks_to_complete_before_resolving() {
    let spawner = Arc::new(Spawner::from_current().unwrap());
    let stream = delayed_task_stream(spawner, Duration::from_secs(3), 1, |spawner| {
        spawner.spawn(async {
            link_async::sleep(Duration::from_secs(1)).await;
            1
        })
    })
    .boxed();
    let start = std::time::SystemTime::now();
    tasks::run_until_idle(stream, Duration::from_secs(1)).await;
    let finish = std::time::SystemTime::now();
    // we started a task, which took 1 second to complete and the idle timeout is 1
    // second. So the total time to complete should be about 2 seconds
    let runtime = finish.duration_since(start).unwrap();
    assert_eq!(runtime.as_secs(), 2);
}

#[tokio::test]
async fn try_run_forever_completes_if_error() {
    let stream = finite_stream(vec![Ok(1), Ok(2), Err(3), Ok(4)]);
    match tasks::try_run_forever(stream).await {
        Ok(_) => panic!("should have errored"),
        Err((error, remaining)) => {
            assert_eq!(error, Err(3));
            let remaining = remaining
                .collect::<Vec<Result<Result<i32, i32>, link_async::JoinError>>>()
                .await
                .into_iter()
                .map(|e| e.unwrap().unwrap())
                .collect::<Vec<i32>>();
            assert_eq!(remaining, vec![4]);
        },
    }
}

#[tokio::test]
async fn try_run_until_idle_completes_if_error() {
    let stream = finite_stream(vec![Ok(1), Ok(2), Err(3), Ok(4)]);
    match tasks::try_run_until_idle(stream, Duration::from_secs(1)).await {
        Ok(_) => panic!("should have errored"),
        Err((error, remaining)) => {
            assert_eq!(error, Err(3));
            let remaining = remaining
                .collect::<Vec<Result<Result<i32, i32>, link_async::JoinError>>>()
                .await
                .into_iter()
                .map(|e| e.unwrap().unwrap())
                .collect::<Vec<i32>>();
            assert_eq!(remaining, vec![4]);
        },
    }
}
