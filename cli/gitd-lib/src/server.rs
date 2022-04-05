// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{io::ErrorKind, panic, process::ExitStatus, sync::Arc};

use async_trait::async_trait;
use futures::{FutureExt, Stream, StreamExt};
use lnk_thrussh as thrussh;
use lnk_thrussh_keys as thrussh_keys;
use rand::Rng;
use tokio::net::{TcpListener, TcpStream};
use tracing::instrument;

use librad::{git::Urn, PeerId};
use link_async::{incoming::TcpListenerExt, Spawner};
use link_git::service;

use crate::{
    hooks::Hooks,
    processes::{ProcessReply, ProcessesHandle},
};

#[derive(Clone)]
pub(crate) struct Server {
    spawner: Arc<Spawner>,
    peer: PeerId,
    processes_handle: ProcessesHandle<ChannelAndSessionId, ChannelHandle>,
    hooks: Hooks,
}

/// The ID of the "extended data" channel in the SSH protocol which corresponds
/// to stderr
const STDERR_ID: u32 = 1;

impl Server {
    pub(crate) fn new(
        spawner: Arc<Spawner>,
        peer: PeerId,
        processes_handle: ProcessesHandle<ChannelAndSessionId, ChannelHandle>,
        hooks: Hooks,
    ) -> Self {
        Self {
            spawner,
            peer,
            processes_handle,
            hooks,
        }
    }

    #[instrument(skip(self, socket, conf))]
    pub(crate) async fn serve(
        self,
        socket: &TcpListener,
        conf: Arc<thrussh::server::Config>,
    ) -> impl Stream<Item = link_async::Task<()>> + '_ {
        let incoming = socket.incoming();
        incoming
            .map(move |stream| match stream {
                Ok(stream) => Some(run_stream(
                    conf.clone(),
                    self.spawner.clone(),
                    self.peer,
                    self.hooks.clone(),
                    self.processes_handle.clone(),
                    stream,
                )),
                Err(e) => {
                    tracing::error!(err=?e, "error accepting incoming connection");
                    None
                },
            })
            .take_while(|e| futures::future::ready(e.is_some()))
            .filter_map(futures::future::ready)
    }
}

#[instrument(skip(conf, spawner, handle, stream, hooks))]
fn run_stream(
    conf: Arc<thrussh::server::Config>,
    spawner: Arc<link_async::Spawner>,
    peer: librad::PeerId,
    hooks: Hooks,
    handle: ProcessesHandle<ChannelAndSessionId, ChannelHandle>,
    stream: TcpStream,
) -> link_async::Task<()> {
    spawner.spawn(async move {
        let handler_stream = thrussh::server::run_stream(
            conf.clone(),
            stream,
            SshHandler {
                peer,
                id: SessionId::random(),
                handle: handle.clone(),
                hooks,
            },
        );
        match handler_stream.await {
            Ok(_) => {
                tracing::info!("server processes disconnected");
            },
            Err(e) if e.is_early_eof() => {
                tracing::warn!("unexpected EOF");
            },
            Err(e) => {
                panic!("error handling SSH: {}", e);
            },
        };
    })
}

struct SshHandler {
    peer: librad::PeerId,
    id: SessionId,
    handle: crate::processes::ProcessesHandle<ChannelAndSessionId, ChannelHandle>,
    hooks: Hooks,
}

impl SshHandler {
    fn channel_id(&self, channel: thrussh::ChannelId) -> ChannelAndSessionId {
        ChannelAndSessionId::new(channel, self.id)
    }
}

#[derive(thiserror::Error, Debug)]
enum HandleError {
    #[error(transparent)]
    Thrussh(#[from] thrussh::Error),
    #[error("failed to exec git: {0}")]
    ExecGit(String),
    #[error("failed to send data to git processes: {0}")]
    SendData(String),
}

impl HandleError {
    fn is_early_eof(&self) -> bool {
        matches!(self, Self::Thrussh(thrussh::Error::IO(io)) if io.kind() == ErrorKind::UnexpectedEof)
    }
}

impl thrussh::server::Handler for SshHandler {
    type Error = HandleError;
    type FutureAuth = futures::future::Ready<Result<(Self, thrussh::server::Auth), HandleError>>;
    type FutureUnit = std::pin::Pin<
        Box<
            dyn futures::Future<Output = Result<(Self, thrussh::server::Session), HandleError>>
                + Send
                + 'static,
        >,
    >;
    type FutureBool =
        futures::future::Ready<Result<(Self, thrussh::server::Session, bool), HandleError>>;

    fn finished_auth(self, auth: thrussh::server::Auth) -> Self::FutureAuth {
        futures::future::ready(Ok((self, auth)))
    }

    fn finished_bool(self, b: bool, session: thrussh::server::Session) -> Self::FutureBool {
        futures::future::ready(Ok((self, session, b)))
    }

    fn finished(self, session: thrussh::server::Session) -> Self::FutureUnit {
        futures::future::ready(Ok((self, session))).boxed()
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn auth_publickey(
        self,
        _user: &str,
        public_key: &thrussh_keys::key::PublicKey,
    ) -> Self::FutureAuth {
        let thrussh_keys::key::PublicKey::Ed25519(k) = public_key;
        let client_key_bytes: &[u8] = &k.key;
        let peer_key_bytes: &[u8] = self.peer.as_ref();
        let auth = if client_key_bytes == peer_key_bytes {
            thrussh::server::Auth::Accept
        } else {
            thrussh::server::Auth::Reject
        };
        self.finished_auth(auth)
    }

    fn data(
        self,
        channel: thrussh::ChannelId,
        data: &[u8],
        session: thrussh::server::Session,
    ) -> Self::FutureUnit {
        let data_vec = data.to_vec();
        async move {
            match self.handle.send(self.channel_id(channel), data_vec).await {
                Ok(_) => Ok((self, session)),
                Err(e) => Err(HandleError::SendData(e.to_string())),
            }
        }
        .boxed()
    }

    fn channel_open_session(
        self,
        _channel: thrussh::ChannelId,
        session: thrussh::server::Session,
    ) -> Self::FutureUnit {
        self.finished(session)
    }

    fn channel_close(
        self,
        channel: thrussh::ChannelId,
        session: thrussh::server::Session,
    ) -> Self::FutureUnit {
        tracing::info!(?channel, "channel close received");
        self.finished(session)
    }

    fn channel_eof(
        self,
        channel: thrussh::ChannelId,
        session: thrussh::server::Session,
    ) -> Self::FutureUnit {
        tracing::info!(?channel, "channel eof received");
        async move {
            match self.handle.eof(self.channel_id(channel)).await {
                Ok(_) => Ok((self, session)),
                Err(e) => Err(HandleError::SendData(e.to_string())),
            }
        }
        .boxed()
    }

    fn signal(
        self,
        channel: thrussh::ChannelId,
        signal_name: thrussh::Sig,
        session: thrussh::server::Session,
    ) -> Self::FutureUnit {
        tracing::info!(?channel, ?signal_name, "received signal");
        if let Some(sig) = nix_signal(&signal_name) {
            async move {
                match self.handle.signal(self.channel_id(channel), sig).await {
                    Ok(_) => Ok((self, session)),
                    Err(e) => Err(HandleError::SendData(e.to_string())),
                }
            }
            .boxed()
        } else {
            tracing::warn!(?signal_name, "unknown signal");
            self.finished(session)
        }
    }

    #[tracing::instrument(level = "debug", skip(self, data, session))]
    fn exec_request(
        self,
        channel: thrussh::ChannelId,
        data: &[u8],
        mut session: thrussh::server::Session,
    ) -> Self::FutureUnit {
        let exec_str = String::from_utf8_lossy(data);
        tracing::debug!(?exec_str, "received exec_request");
        let ssh_service: service::SshService<Urn> = match exec_str.parse() {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(err=?e, ?exec_str, "unable to parse exec str for exec_request");
                session.extended_data(
                    channel,
                    STDERR_ID,
                    "invalid exec request str".to_string().into(),
                );
                session.channel_failure(channel);
                return self.finished(session);
            },
        };
        tracing::debug!(%ssh_service.service, %ssh_service.path, "parsed exec_request");

        let id = self.channel_id(channel);
        let handle = ChannelHandle::new(session.handle(), channel);
        async move {
            match self
                .handle
                .exec_git(id, handle, ssh_service, self.hooks.clone())
                .await
            {
                Ok(_) => {
                    session.channel_success(channel);
                    Ok((self, session))
                },
                Err(e) => Err(HandleError::ExecGit(e.to_string())),
            }
        }
        .boxed()
    }
}

#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub(crate) struct SessionId([u8; 32]);

impl std::fmt::Debug for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionId(")?;
        for byte in &self.0 {
            write!(f, "{:X}", byte)?
        }
        f.write_str(")")
    }
}

impl SessionId {
    fn random() -> Self {
        let mut rng = rand::thread_rng();
        let raw: [u8; 32] = rng.gen();
        Self(raw)
    }
}

/// A combination of channel and session ID which uniquely identifies a channel
/// in this SSH server
#[derive(Hash, Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChannelAndSessionId {
    channel: thrussh::ChannelId,
    session: SessionId,
}

impl ChannelAndSessionId {
    fn new(channel: thrussh::ChannelId, session: SessionId) -> Self {
        Self { channel, session }
    }
}

/// A handle which implements [`ProcessReply`] by sending data to the given
/// channel on the given `thrussh::Handle`
#[derive(Clone)]
pub(crate) struct ChannelHandle {
    handle: thrussh::server::Handle,
    channel_id: thrussh::ChannelId,
}

impl ChannelHandle {
    fn new(handle: thrussh::server::Handle, channel_id: thrussh::ChannelId) -> Self {
        Self { handle, channel_id }
    }
}

#[derive(thiserror::Error, Debug)]
#[error("failed to send to thrussh::server::Handle")]
pub(crate) struct ReplyError;

#[async_trait]
impl ProcessReply for ChannelHandle {
    type Error = ReplyError;

    async fn stdout_data(&mut self, data: Vec<u8>) -> Result<(), Self::Error> {
        self.handle
            .data(self.channel_id, data.into())
            .await
            .map_err(|_| ReplyError)
    }

    async fn stderr_data(&mut self, data: Vec<u8>) -> Result<(), Self::Error> {
        self.handle
            .extended_data(self.channel_id, STDERR_ID, data.into())
            .await
            .map_err(|_| ReplyError)
    }

    async fn exit_status(&mut self, exit_status: ExitStatus) -> Result<(), Self::Error> {
        if let Some(code) = exit_status.code() {
            tracing::trace!(?code, "process exited with exit code");
            self.handle
                .exit_status_request(self.channel_id, code as u32)
                .await
                .map_err(|_| ReplyError)?;
        } else {
            tracing::trace!("process was killed by signal");
            // Should figure out how to map the signal to thrussh::Sig but it's a lot of
            // work
            self.handle
                .exit_signal_request(
                    self.channel_id,
                    thrussh::Sig::KILL,
                    false,
                    "killed".to_string(),
                    "".to_string(),
                )
                .await
                .map_err(|_| ReplyError)?;
        }
        Ok(())
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        self.handle
            .eof(self.channel_id)
            .await
            .map_err(|_| ReplyError)?;
        self.handle
            .close(self.channel_id)
            .await
            .map_err(|_| ReplyError)
    }
}

fn nix_signal(sig: &thrussh::Sig) -> Option<nix::sys::signal::Signal> {
    use nix::sys::signal::Signal;
    use thrussh::Sig;
    match sig {
        Sig::ABRT => Some(Signal::SIGABRT),
        Sig::ALRM => Some(Signal::SIGALRM),
        Sig::FPE => Some(Signal::SIGFPE),
        Sig::HUP => Some(Signal::SIGHUP),
        Sig::ILL => Some(Signal::SIGILL),
        Sig::INT => Some(Signal::SIGINT),
        Sig::KILL => Some(Signal::SIGKILL),
        Sig::PIPE => Some(Signal::SIGPIPE),
        Sig::QUIT => Some(Signal::SIGQUIT),
        Sig::SEGV => Some(Signal::SIGSEGV),
        Sig::TERM => Some(Signal::SIGTERM),
        Sig::USR1 => Some(Signal::SIGUSR1),
        Sig::Custom(_) => None,
    }
}
