// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{collections::HashMap, ffi::OsStr, fmt, path::PathBuf, str::FromStr, time::Duration};

use futures::{future::BoxFuture, stream::FuturesUnordered, FutureExt, Stream, StreamExt as _};
use multihash::Multihash;
use tokio::sync::mpsc;

use link_identities::urn::HasProtocol;

use super::{Data, Display, Track};

pub mod config;
pub use config::Config;

/// End of transimission character.
pub const EOT: u8 = 0x04;

/// A notification sent by the notifying process to the set of hook processes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notification<R> {
    Track(Track<R>),
    Data(Data<R>),
}

impl<R> fmt::Display for Notification<R>
where
    R: HasProtocol + fmt::Display,
    for<'a> &'a R: Into<Multihash>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Track(track) => write!(f, "{}", track),
            Self::Data(data) => write!(f, "{}", data),
        }
    }
}

impl<R> From<Track<R>> for Notification<R> {
    fn from(t: Track<R>) -> Self {
        Self::Track(t)
    }
}

impl<R> From<Data<R>> for Notification<R> {
    fn from(t: Data<R>) -> Self {
        Self::Data(t)
    }
}

/// Executor for a set of [`Hook`]s that will receive and process
/// [`Notification`]s via a channel.
pub struct Hooks<P: Process> {
    data_hooks: Vec<Hook<P>>,
    track_hooks: Vec<Hook<P>>,
    config: Config,
}

impl<P: Process + Send + Sync + 'static> Hooks<P> {
    /// Construct the `Hooks` runner.
    ///
    /// To create a [`Hook`], use the [`Process::spawn`] constructor, where the
    /// child process of the `Hook` must also implement [`Process`].
    ///
    /// To start the hooks routine and process [`Notification`]s, use
    /// [`Hooks::run`].
    pub fn new(config: Config, data_hooks: Vec<Hook<P>>, track_hooks: Vec<Hook<P>>) -> Self {
        Self {
            data_hooks,
            track_hooks,
            config,
        }
    }

    /// The `incoming` [`Notification`]s are sent to each respective hook,
    /// depending on the notification variant, until the stream is exhausted.
    ///
    /// Once the stream is complete, the end-of-transmission character is sent
    /// to every hook to signal that they should stop. The hook is given a
    /// grace period to stop and exit, otherwise it will be terminated after the
    /// timeout given in the [`Config`].
    pub async fn run<S, R>(self, mut incoming: S)
    where
        R: Clone + HasProtocol + std::fmt::Display + Send + Sync + 'static,
        for<'b> &'b R: Into<Multihash>,
        S: Stream<Item = Notification<R>> + Unpin,
    {
        use senders::{Event, Senders};

        let mut routines = FuturesUnordered::new();
        let mut data_senders: Senders<Data<R>> = Senders::new(Event::Data);
        let mut track_senders: Senders<Track<R>> = Senders::new(Event::Track);

        for hook in self.data_hooks {
            let path = hook.path.clone();
            tracing::debug!(hook = %path.display(), "starting data hook");
            let (sender, routine) = hook.start(self.config.hook);
            data_senders.insert(path, sender);
            routines.push(routine);
        }
        for hook in self.track_hooks {
            let path = hook.path.clone();
            tracing::debug!(hook = %path.display(), "starting track hook");
            let (sender, routine) = hook.start(self.config.hook);
            track_senders.insert(path, sender);
            routines.push(routine);
        }
        loop {
            futures::select! {
                failed_hook_path = routines.next().fuse() => {
                    if let Some(failed_hook_path) = failed_hook_path {
                        tracing::warn!(hook = %failed_hook_path.display(), "hook failed, removing from hooks set");
                        data_senders.remove(&failed_hook_path);
                        track_senders.remove(&failed_hook_path);
                    } else {
                        tracing::error!("all hook routines have stopped");
                        break;
                    }
                }
                n = incoming.next().fuse() => {
                    match n {
                        Some(Notification::Data(d)) => {
                            tracing::trace!(data = %d, "received data notification");
                            data_senders.send(d)
                        },
                        Some(Notification::Track(t)) => {
                            tracing::trace!(track = %t, "received track notification");
                            track_senders.send(t)
                        },
                        None => {
                            tracing::trace!("finished notifications stream");
                            break
                        },
                    }
                },
            }
        }

        // Send EOTs to all senders
        data_senders.eot().await;
        track_senders.eot().await;

        // Wait for routines to complete
        for routine in routines {
            let path = routine.await;
            tracing::info!(hook = %path.display(), "hook finished");
        }
    }
}

/// A communication medium for a hook process.
///
/// # Cancel Safety
///
/// Since the cancel safety is based on the implementing data type of `Handle`,
/// it should be assumed that the methods are *not* cancel safe.
#[async_trait]
pub trait Process: Sized {
    type SpawnError: std::error::Error + Send + Sync + 'static;
    type WriteError: std::error::Error + Send + Sync + 'static;
    type DieError: std::error::Error + Send + Sync + 'static;

    /// Spawn a new hook process where `path` points to the hook executable. The
    /// `args` should typically be `None::<String>`, but can be used for testing
    /// purposes.
    async fn spawn<I, S>(path: PathBuf, args: I) -> Result<Self, Self::SpawnError>
    where
        I: IntoIterator<Item = S> + Send,
        S: AsRef<OsStr>;

    /// Write data to the hook process.
    async fn write(&mut self, bs: &[u8]) -> Result<(), Self::WriteError>;

    /// Wait for the hook process to finish, or kill after `duration`.
    async fn wait_or_kill(&mut self, duration: Duration) -> Result<(), Self::DieError>;
}

/// A spawned hook process.
pub struct Hook<P: Process> {
    path: PathBuf,
    child: P,
}

pub enum HookMessage<T> {
    /// End of transmission message.
    EOT,
    /// The payload to be sent to a hook, usually [`Data`] or [`Track`].
    Payload(T),
}

impl<T> From<T> for HookMessage<T> {
    fn from(t: T) -> Self {
        Self::Payload(t)
    }
}

impl<T: FromStr> FromStr for HookMessage<T> {
    type Err = T::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == String::from_utf8(vec![EOT]).expect("BUG: EOT is valid utf-8") {
            Ok(Self::EOT)
        } else {
            s.parse().map(Self::Payload)
        }
    }
}

impl<P: Process + Send + Sync + 'static> Hook<P> {
    pub fn new(path: PathBuf, child: P) -> Self {
        Self { path, child }
    }

    #[tracing::instrument(skip(self), fields(hook = ?self.path))]
    pub fn start<'a, D>(
        mut self,
        config: config::Hook,
    ) -> (mpsc::Sender<HookMessage<D>>, BoxFuture<'a, PathBuf>)
    where
        D: Display + Send + Sync + 'static,
    {
        let (sx, mut rx) = mpsc::channel::<HookMessage<D>>(config.buffer);
        let routine = async move {
            tracing::trace!("waiting for notification");
            while let Some(msg) = rx.recv().await {
                match msg {
                    HookMessage::EOT => {
                        if let Err(err) = self.write(&[EOT]).await {
                            tracing::warn!(err = %err, "failed to write EOT to hook");
                        }
                        if let Err(err) = self.wait_or_kill(config.timeout).await {
                            tracing::warn!(err = %err, "failed to terminate hook");
                        }
                        return self.path;
                    },
                    HookMessage::Payload(msg) => {
                        if let Err(err) = self.write(msg.display().as_bytes()).await {
                            tracing::warn!(err = %err, "failed to write to hook");
                            return self.path;
                        }
                    },
                }
            }
            self.path
        }
        .boxed();
        (sx, routine)
    }
}

#[async_trait]
impl<P> Process for Hook<P>
where
    P: Process + Send + Sync + 'static,
{
    type WriteError = P::WriteError;
    type SpawnError = P::SpawnError;
    type DieError = P::DieError;

    async fn spawn<I, S>(path: PathBuf, args: I) -> Result<Self, Self::SpawnError>
    where
        I: IntoIterator<Item = S> + Send,
        S: AsRef<OsStr>,
    {
        Ok(Self {
            path: path.clone(),
            child: P::spawn(path, args).await?,
        })
    }

    async fn write(&mut self, bs: &[u8]) -> Result<(), Self::WriteError> {
        self.child.write(bs).await
    }

    async fn wait_or_kill(&mut self, duration: Duration) -> Result<(), Self::DieError> {
        self.child.wait_or_kill(duration).await
    }
}

pub(super) mod senders {
    use super::*;

    #[derive(Debug)]
    pub enum Event {
        Track,
        Data,
    }

    pub struct Senders<P> {
        senders: HashMap<PathBuf, mpsc::Sender<HookMessage<P>>>,
        kind: Event,
    }

    impl<P> Senders<P> {
        pub fn new(kind: Event) -> Self {
            Self {
                senders: HashMap::new(),
                kind,
            }
        }

        pub fn insert(&mut self, path: PathBuf, sender: mpsc::Sender<HookMessage<P>>) {
            self.senders.insert(path, sender);
        }

        pub fn remove(&mut self, path: &PathBuf) {
            self.senders.remove(path);
        }

        pub fn send(&self, p: P)
        where
            P: Clone,
        {
            for (path, sender) in self.senders.iter() {
                if sender.try_send(p.clone().into()).is_err() {
                    tracing::warn!(hook=%path.display(), kind=?self.kind, "dropping message for hook which is running too slowly");
                }
            }
        }

        pub async fn eot(&self) {
            for (path, sender) in self.senders.iter() {
                if let Err(err) = sender.send(HookMessage::EOT).await {
                    tracing::warn!(hook=%path.display(), kind=?self.kind, err=%err, "failed to send EOT");
                }
            }
        }
    }
}

mod tokio_impl {
    use std::{ffi::OsStr, io, path::PathBuf, process::Stdio, time::Duration};
    use tokio::{
        io::AsyncWriteExt,
        process::{Child, Command},
    };

    use super::Process;

    #[async_trait]
    impl Process for Child {
        type WriteError = io::Error;
        type SpawnError = io::Error;
        type DieError = io::Error;

        async fn spawn<I, S>(path: PathBuf, args: I) -> Result<Self, Self::SpawnError>
        where
            I: IntoIterator<Item = S> + Send,
            S: AsRef<OsStr>,
        {
            // TODO: figure out how to pipe stdout/stderr to tracing
            let child = Command::new(path)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .args(args)
                .spawn()?;
            Ok(child)
        }

        async fn write(&mut self, bs: &[u8]) -> Result<(), Self::WriteError> {
            self.stdin
                .as_mut()
                .expect("BUG: stdin was not set up for subprocess")
                .write_all(bs)
                .await
        }

        async fn wait_or_kill(&mut self, duration: Duration) -> Result<(), Self::DieError> {
            if tokio::time::timeout(duration, self.wait()).await.is_err() {
                self.kill().await
            } else {
                Ok(())
            }
        }
    }
}
