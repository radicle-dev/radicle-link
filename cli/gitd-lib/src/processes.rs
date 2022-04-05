// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! This module is an implementation of a pool of running git subprocesses where
//! client code is responsible for delivering incoming standard input data to
//! the pool and delivering outgoing standard output and standard error data to
//! users.
//!
//! The intended use is to create an instance of [`Processes`] and
//! [`ProcessesHandle`] using [`Processes::new`], then run the handler loop
//! using [`Processes::run`] whilst submitting new tasks and new incoming data
//! using the `ProcessesHandle` and eventually terminating the `Processes` loop
//! with [`ProcessesHandle::stop`].
//!
//! Incoming data is labelled by the ID of the stream it is arriving on, replies
//! are sent directly from the subprocess via the `ProcessReply` trait.

use std::{collections::HashMap, fmt::Debug, hash::Hash, panic, process::ExitStatus, sync::Arc};

use async_trait::async_trait;
use futures::{
    select,
    stream::{FuturesUnordered, StreamExt},
    FutureExt,
};
use librad::git::{
    storage::{pool::Pool, Storage},
    Urn,
};
use link_async::{Spawner, Task};
use link_git::service::SshService;
use tracing::instrument;

use crate::{git_subprocess, hooks::Hooks};

const MAX_IN_FLIGHT_GITS: usize = 10;

/// A trait representing a channel for data to be sent from a running process to
/// the user.
#[async_trait]
pub(crate) trait ProcessReply {
    type Error: std::error::Error + Send + 'static;
    /// Data to be delivered to the users standard input
    async fn stdout_data(&mut self, data: Vec<u8>) -> Result<(), Self::Error>;

    /// Data to be delivered to the users standard error
    async fn stderr_data(&mut self, data: Vec<u8>) -> Result<(), Self::Error>;

    /// Notify the user that the process exited with the given `ExitStatus`
    async fn exit_status(&mut self, status: ExitStatus) -> Result<(), Self::Error>;

    /// Notify the user that this channel is closing
    async fn close(&mut self) -> Result<(), Self::Error>;
}

/// The type of messages which the `ProcessesHandle` sends to the `Processes`
/// run loop
enum Message<Id> {
    /// A message to be sent to the subprocessed identified by `Id`
    Message(Id, git_subprocess::Message),
    /// Attempt to shutdown, waiting for any running processes to stop
    Stop,
}

/// The message which `ProcessesHandle` sends to the `Processes` loop to start a
/// new git subprocess. This is separate to the `Incoming` type because it is
/// sent on a separate channel, which allows us to exert backpressure on
/// incoming exec requests.
struct ExecGit<Id, Reply> {
    service: SshService<Urn>,
    channel: Id,
    handle: Reply,
    hooks: Hooks,
}

/// The control interface for the `Processes` loop
///
/// All the methods on this struct return a `ProcessesLoopGone` error if they
/// fail to send a control message to the processes loop. This error (as it's
/// name suggests) occurs if the receiving end of the message channel the
/// `ProcessesHandle` wraps has been dropped or closed. This most likely
/// indicates that there has been an error in the `Processes::run` loop.
#[derive(Clone)]
pub(crate) struct ProcessesHandle<Id, Reply> {
    sender: tokio::sync::mpsc::Sender<Message<Id>>,
    exec_git_send: tokio::sync::mpsc::Sender<ExecGit<Id, Reply>>,
}

#[derive(thiserror::Error, Debug)]
#[error("unable to send message to processes loop, the receiver has gone")]
pub(crate) struct ProcessesLoopGone;

impl<Id: Debug, Reply> ProcessesHandle<Id, Reply> {
    /// Begin a new git subprocess. Any data delivered via
    /// `ProcessesHandle::data` for the `channel` passed here will be
    /// delivered to the subprocess which is started as a result of
    /// this call. All data from the standard output and standard error, and the
    /// exit status of the subprocess will be delivered to the `Reply`
    /// implementation in `handle`.
    ///
    /// There is a cap on the number of concurrent git processees which may be
    /// running. If that cap is reached then this method will wait until a
    /// running process has finished before starting a new process and
    /// returning a success.
    #[instrument(skip(self, service, handle, hooks))]
    pub(crate) async fn exec_git(
        &self,
        channel: Id,
        handle: Reply,
        service: SshService<Urn>,
        hooks: Hooks,
    ) -> Result<(), ProcessesLoopGone> {
        self.exec_git_send
            .send(ExecGit {
                channel,
                handle,
                service,
                hooks,
            })
            .await
            .map_err(|_| ProcessesLoopGone)
    }

    /// Deliver data for the standard input of the process identified by `id`
    pub(crate) async fn send(&self, id: Id, data: Vec<u8>) -> Result<(), ProcessesLoopGone> {
        self.sender
            .send(Message::Message(id, git_subprocess::Message::Data(data)))
            .await
            .map_err(|_| ProcessesLoopGone)
    }

    pub(crate) async fn eof(&self, id: Id) -> Result<(), ProcessesLoopGone> {
        self.sender
            .send(Message::Message(id, git_subprocess::Message::Eof))
            .await
            .map_err(|_| ProcessesLoopGone)
    }

    pub(crate) async fn signal(
        &self,
        id: Id,
        sig: nix::sys::signal::Signal,
    ) -> Result<(), ProcessesLoopGone> {
        self.sender
            .send(Message::Message(id, git_subprocess::Message::Signal(sig)))
            .await
            .map_err(|_| ProcessesLoopGone)
    }

    /// Signal to the `Processes` loop that it should stop.
    pub(crate) async fn stop(&self) -> Result<(), ProcessesLoopGone> {
        self.sender
            .send(Message::Stop)
            .await
            .map_err(|_| ProcessesLoopGone)
    }
}

type GitProcessResult<Id, E> = (Id, Result<(), git_subprocess::Error<E>>);

pub(crate) struct Processes<Id, Reply: ProcessReply> {
    spawner: Arc<Spawner>,
    pool: Arc<Pool<Storage>>,
    /// Incoming control messages
    incoming: tokio::sync::mpsc::Receiver<Message<Id>>,
    /// Incoming exec git requests
    exec_git_incoming: tokio::sync::mpsc::Receiver<ExecGit<Id, Reply>>,
    /// Hashmap from process ID (as passed in ExecGit) to the sender which
    /// connects to the std input of the running subprocess.
    process_sends: HashMap<Id, tokio::sync::mpsc::Sender<git_subprocess::Message>>,
    /// The running git subprocesses
    running_processes: FuturesUnordered<Task<GitProcessResult<Id, Reply::Error>>>,
    /// If we are waiting for running processes to stop before exiting
    stopping: bool,
}

#[derive(thiserror::Error, Debug)]
pub(crate) enum ProcessRunError<Id> {
    #[error("attempted to send to subprocess id {0} but the receiver is gone")]
    SubprocessDisappeared(Id),
}

impl<Id, Reply> Processes<Id, Reply>
where
    Id: Debug + Clone + Send + Eq + Hash + 'static,
    Reply: ProcessReply + Send + Sync + 'static + Clone,
    Reply::Error: Send + 'static,
{
    pub(crate) fn new(
        spawner: Arc<Spawner>,
        pool: Arc<Pool<Storage>>,
    ) -> (Processes<Id, Reply>, ProcessesHandle<Id, Reply>) {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let (exec_git_tx, exec_git_rx) = tokio::sync::mpsc::channel(1);
        let processes = Processes {
            spawner,
            pool,
            incoming: rx,
            exec_git_incoming: exec_git_rx,
            process_sends: HashMap::new(),
            running_processes: FuturesUnordered::new(),
            stopping: false,
        };
        let handle = ProcessesHandle {
            sender: tx,
            exec_git_send: exec_git_tx,
        };
        (processes, handle)
    }

    #[instrument(skip(self, handle, hooks))]
    fn exec_git(&mut self, id: Id, handle: Reply, service: SshService<Urn>, hooks: Hooks) {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let task = self.spawner.spawn({
            let spawner = self.spawner.clone();
            let pool = self.pool.clone();
            let id = id.clone();
            async move {
                let result =
                    git_subprocess::run_git_subprocess(spawner, pool, rx, handle, service, hooks)
                        .await;
                (id, result)
            }
        });
        self.running_processes.push(task);
        self.process_sends.insert(id, tx);
    }

    #[instrument(skip(self, data))]
    async fn send(&mut self, id: Id, data: Vec<u8>) -> Result<(), ProcessRunError<Id>> {
        if let Some(sender) = self.process_sends.get(&id) {
            sender
                .send(git_subprocess::Message::Data(data))
                .await
                .map_err(|_| ProcessRunError::SubprocessDisappeared(id))
        } else {
            tracing::warn!(channel_id=?id, "received data for unknown channel ID");
            Ok(())
        }
    }

    #[instrument(skip(self))]
    async fn eof(&mut self, id: Id) {
        if let Some(sender) = self.process_sends.get(&id) {
            sender.send(git_subprocess::Message::Eof).await.ok();
        } else {
            tracing::warn!(channel_id=?id, "received eof for unknown channel ID");
        }
    }

    #[instrument(skip(self))]
    async fn signal(&mut self, id: Id, signal: nix::sys::signal::Signal) {
        if let Some(sender) = self.process_sends.get(&id) {
            sender
                .send(git_subprocess::Message::Signal(signal))
                .await
                .ok();
        } else {
            tracing::warn!(channel_id=?id, "received signal for unknown channel ID");
        }
    }

    /// Start the process handling event loop.
    #[instrument(skip(self))]
    pub(crate) async fn run(mut self) -> Result<(), ProcessRunError<Id>> {
        loop {
            let next_git_command =
                if (self.running_processes.len() > MAX_IN_FLIGHT_GITS) || self.stopping {
                    futures::future::Fuse::terminated()
                } else {
                    self.exec_git_incoming.recv().boxed().fuse()
                };
            let finished_processes = &mut self.running_processes;
            if self.stopping && finished_processes.is_empty() {
                return Ok(());
            }
            futures::pin_mut!(finished_processes);
            select! {
                completed_task = finished_processes.next() => self.handle_completed(completed_task),
                next_exec_git = next_git_command.fuse() => {
                    if let Some(ExecGit{service, channel, handle, hooks}) = next_exec_git {
                        self.exec_git(channel, handle, service, hooks);
                    }
                },
                new_incoming = self.incoming.recv().fuse() => self.handle_incoming(new_incoming).await?,
            }
        }
    }

    fn handle_completed(
        &mut self,
        completed_task: Option<Result<GitProcessResult<Id, Reply::Error>, link_async::JoinError>>,
    ) {
        match completed_task {
            Some(Ok((id, result))) => {
                self.process_sends.remove(&id);
                match result {
                    Ok(()) => {
                        tracing::info!(id=?id, "task finished");
                    },
                    Err(e) => {
                        use git_subprocess::Error::*;
                        match e {
                            Reply(_) => {
                                tracing::warn!("subprocess terminated because client disappeared")
                            },
                            Unexpected(e) => tracing::error!(err=?e, "subprocess failed"),
                        }
                    },
                }
            },
            Some(Err(e)) => {
                if e.is_panic() {
                    tracing::error!(err=?e, "panic encountered in subprocess");
                    panic::resume_unwind(Box::new(e))
                } else {
                    panic!("task cancelled whilst held by processes");
                }
            },
            None => (),
        }
    }

    async fn handle_incoming(
        &mut self,
        new_incoming: Option<Message<Id>>,
    ) -> Result<(), ProcessRunError<Id>> {
        if let Some(new_incoming) = new_incoming {
            use git_subprocess::Message::*;
            match new_incoming {
                Message::Message(channel, Data(data)) => {
                    tracing::trace!(?channel, "data received");
                    self.send(channel, data).await?;
                },
                Message::Message(channel, Eof) => {
                    tracing::trace!(?channel, "eof received");
                    self.eof(channel).await;
                },
                Message::Message(channel, Signal(signal)) => {
                    tracing::trace!(?channel, ?signal, "signal received");
                    self.signal(channel, signal).await;
                },
                Message::Stop => {
                    tracing::trace!("stopping subprocesses");
                    self.stopping = true;
                },
            }
        }
        Ok(())
    }
}
