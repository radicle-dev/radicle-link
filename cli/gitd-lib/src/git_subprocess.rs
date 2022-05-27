// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    fmt::Debug,
    os::unix::process::ExitStatusExt,
    process::{ExitStatus, Stdio},
    sync::Arc,
};

use futures::{
    future::{Fuse, FusedFuture},
    FutureExt,
};
use git2::transport::Service as GitService;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Child,
};

use librad::git::storage;
use link_async::Spawner;

use crate::{
    hooks::{self, Hooks},
    processes::ProcessReply,
    ssh_service,
};

pub mod command;

pub(crate) enum Message {
    Signal(nix::sys::signal::Signal),
    Data(Vec<u8>),
    Eof,
}

#[derive(thiserror::Error, Debug)]
pub enum Error<ReplyError> {
    #[error("unexpected error when running git subprocess: {0}")]
    Unexpected(Box<dyn std::error::Error + Send + 'static>),
    #[error("unable to send reply to client: {0}")]
    Reply(ReplyError),
}

#[tracing::instrument(level = "trace", skip(spawner, pool, incoming, out, hooks))]
pub(crate) async fn run_git_subprocess<Replier, S>(
    spawner: Arc<Spawner>,
    pool: Arc<storage::Pool<storage::Storage>>,
    incoming: tokio::sync::mpsc::Receiver<Message>,
    mut out: Replier,
    service: ssh_service::SshService,
    hooks: Hooks<S>,
) -> Result<(), Error<Replier::Error>>
where
    Replier: ProcessReply + Clone,
    S: librad::Signer + Clone,
{
    let result = run_git_subprocess_inner(spawner, pool, incoming, &mut out, service, hooks).await;
    match out.close().await {
        Ok(()) => {},
        Err(e) => {
            tracing::error!(err=?e, "error trying to close channel");
        },
    }
    result
}

#[tracing::instrument(level = "trace", skip(spawner, pool, incoming, out, hooks))]
async fn run_git_subprocess_inner<Replier, S>(
    spawner: Arc<Spawner>,
    pool: Arc<storage::Pool<storage::Storage>>,
    mut incoming: tokio::sync::mpsc::Receiver<Message>,
    out: &mut Replier,
    service: ssh_service::SshService,
    hooks: Hooks<S>,
) -> Result<(), Error<Replier::Error>>
where
    Replier: ProcessReply + Clone,
    S: librad::Signer + Clone,
{
    let mut progress_reporter = Reporter {
        replier: out.clone(),
    };

    if service.is_upload() {
        match hooks
            .pre_upload(&mut progress_reporter, service.path.clone().into())
            .await
        {
            Ok(()) => {},
            Err(hooks::error::Progress(err)) => {
                tracing::error!(err=%err, "failed pre-receive hook");
                return Ok(());
            },
        }
    }

    let mut git = {
        let storage = pool.get().await.map_err(|e| {
            tracing::error!(err=?e, "error opening storage pool");
            Error::Unexpected(Box::new(e))
        })?;
        let service = service.clone();
        spawner
            .blocking::<_, Result<_, _>>(move || command::create_command(&storage, service))
            .await
            .map_err(|e| {
                tracing::error!(err=?e, "error creating git subcommand");
                Error::Unexpected(Box::new(e))
            })?
    };

    let mut child = match git
        .arg(".")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(err=?e, "error spawning git subprocess");
            out.stderr_data(
                format!("error spawning git subprocess: {}\n", e)
                    .as_bytes()
                    .to_vec(),
            )
            .await
            .map_err(Error::Reply)?;
            return Ok(());
        },
    };

    let mut child_stdin = Some(child.stdin.take().unwrap());
    let mut child_stdout = child.stdout.take().unwrap();
    let mut child_stderr = child.stderr.take().unwrap();

    let mut stdout_buffer = [0; 1000];
    let mut stderr_buffer = [0; 1000];
    let exit_status = loop {
        futures::select! {
            input = incoming.recv().fuse() => {
                match input {
                    Some(Message::Data(bytes)) => {
                         if let Some(ref mut child_stdin) = child_stdin {
                            if let Err(e) = child_stdin.write_all(&bytes[..]).await {
                                tracing::error!(err=?e, "error sending to child process");
                            };
                         }
                    },
                    Some(Message::Eof) => {
                        // drop stdin to signal to the subprocess that the client is going away
                        if let Some(mut child_stdin) = child_stdin.take() {
                            child_stdin.shutdown().await.map_err(|e|{
                                tracing::error!(err=?e, "error flushing child stdin");
                                Error::Unexpected(Box::new(e))
                            })?;
                        }
                    },
                    Some(Message::Signal(sig)) => {
                        tracing::info!(signal=?sig, "forwarding signal to subprocess");
                        use nix::{unistd::Pid, sys::signal};
                        if let Some(pid) = child.id() {
                            let pid = Pid::from_raw(pid as i32);
                            match signal::kill(pid, sig) {
                                Ok(pid) => {
                                    tracing::trace!(pid=?pid, "signal forwarded");
                                },
                                Err(e) => {
                                    tracing::error!(err=?e, "failed to send signal to subprocess");
                                    out.stderr_data("failed to send signal to subprocess\n".as_bytes().to_vec()).await.ok();
                                }
                            }
                        } else {
                            tracing::error!("no pid for subprocess");
                            out.stderr_data("no PID for subprocess\n".as_bytes().to_vec()).await.ok();
                        }
                    },
                    None => {},
                }
            },
            bytes_read = child_stdout.read(&mut stdout_buffer).fuse() => {
                if !forward_input(bytes_read, &stdout_buffer, |d| out.stdout_data(d.to_vec())).await {
                    kill_child(&mut child).await?;
                    return Ok(());
                }
            },
            err_bytes_read = child_stderr.read(&mut stderr_buffer).fuse() => {
                if !forward_input(err_bytes_read, &stderr_buffer, |d| out.stderr_data(d.to_vec())).await {
                    kill_child(&mut child).await?;
                    return Ok(());
                }
            },
            status = child.wait().fuse() => {
                tracing::trace!(?status, "subprocess completed");
                match status {
                    Ok(s) => break s,
                    Err(e) => {
                        tracing::error!(err=?e, "error reading exit status");
                        out.stderr_data(
                            "unable to determine exit status of git subprocess, closing connection\n"
                                .as_bytes()
                                .to_vec(),
                        )
                        .await.ok();
                        return Err(Error::Unexpected(Box::new(e)));
                    }
                }
            }
        }
    };

    // drain remaining output
    let mut child_stdout = Some(child_stdout);
    let mut child_stderr = Some(child_stderr);
    loop {
        let stdout_bytes = child_stdout
            .as_mut()
            .map(|c| c.read(&mut stdout_buffer).fuse())
            .unwrap_or_else(Fuse::terminated);
        let stderr_bytes = child_stderr
            .as_mut()
            .map(|c| c.read(&mut stderr_buffer).fuse())
            .unwrap_or_else(Fuse::terminated);
        if stdout_bytes.is_terminated() && stderr_bytes.is_terminated() {
            break;
        }
        futures::pin_mut!(stdout_bytes);
        futures::pin_mut!(stderr_bytes);
        futures::select! {
            bytes_read = stdout_bytes => {
                if let Ok(0) = bytes_read {
                    child_stdout = None;
                } else {
                    forward_input(bytes_read, &stdout_buffer, |d| out.stdout_data(d.to_vec())).await;
                }
            },
            err_bytes_read = stderr_bytes => {
                if let Ok(0) = err_bytes_read {
                    child_stderr = None;
                } else {
                    forward_input(err_bytes_read, &stderr_buffer, |d| out.stderr_data(d.to_vec())).await;
                }
            },
        }
    }

    if !exit_status.success() {
        tracing::error!(
            exit_status=?exit_status.code(),
            "non-successful exit status received whilst executing git subprocess"
        );
        out.exit_status(exit_status).await.map_err(Error::Reply)?;
        return Ok(());
    }

    // Run hooks
    if service.service == GitService::ReceivePack.into() {
        if let Err(e) = hooks
            .post_receive(&mut progress_reporter, service.path.into())
            .await
        {
            match e {
                hooks::error::PostReceive::Progress(_)
                | hooks::error::PostReceive::Announce(hooks::error::Announce::Progress(_))
                | hooks::error::PostReceive::Update(hooks::error::UpdateSignedRefs::Progress(_))
                | hooks::error::PostReceive::RequestPull(hooks::error::RequestPull::Progress(_)) =>
                {
                    tracing::error!("client went away whilst executing post receive hook");
                },
                other => {
                    tracing::error!(err=?other, "error executing post receive hook");
                    out.stderr_data(
                        format!("error executing post receive hook: {}\n", other).into_bytes(),
                    )
                    .await
                    .map_err(Error::Reply)?;
                },
            }
        }
    };

    out.exit_status(ExitStatus::from_raw(0))
        .await
        .map_err(Error::Reply)?;

    Ok(())
}

struct Reporter<R> {
    replier: R,
}

impl<R, E> hooks::ProgressReporter for Reporter<R>
where
    E: 'static,
    R: ProcessReply<Error = E>,
{
    type Error = E;

    fn report(
        &mut self,
        progress: hooks::Progress,
    ) -> futures::future::BoxFuture<Result<(), Self::Error>> {
        let message = format!("{}\n", progress).into_bytes();
        self.replier.stderr_data(message).boxed()
    }
}

async fn kill_child<E>(child: &mut Child) -> Result<(), Error<E>> {
    match child.kill().await {
        Ok(_) => {
            tracing::info!("successfully killed subprocess");
            Ok(())
        },
        Err(e) => {
            tracing::error!(err=?e, "unable to kill subprocess");
            Err(Error::Unexpected(Box::new(e)))
        },
    }
}

/// Forward `bytes_read` bytes from `buffer` to the closure `f`. Returns false
/// if `f` returned an error, indicating that the receiver went away whilst we
/// were trying to forward input.
async fn forward_input<
    E: std::error::Error,
    D: futures::Future<Output = Result<(), E>>,
    F: FnOnce(&[u8]) -> D,
>(
    bytes_read: Result<usize, std::io::Error>,
    buffer: &[u8],
    f: F,
) -> bool {
    match bytes_read {
        Ok(bytes_read) => {
            if f(&buffer[0..bytes_read]).await.is_err() {
                tracing::warn!("receiver disappeared whilst subprocess was running");
                false
            } else {
                true
            }
        },
        Err(e) => {
            tracing::error!(err=?e, "error reading from child process");
            true
        },
    }
}
