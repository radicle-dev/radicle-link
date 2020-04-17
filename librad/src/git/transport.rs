// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

//! A custom git transport
//!
//! The `register` function registers a transport which expects URLs of the
//! form:
//!
//! `rad://LOCAL_PEER_ID@REMOTE_PEER_ID/PROJECT_ID`
//!
//! The local peer id is needed to support testing with multiple peers:
//! `libgit2` stores custom transports in a `static` variable, so we can
//! register ours only once per program.
//!
//! # Note
//!
//! The wire protocol of the transport conforms to the one [`git-daemon`]
//! implements. However, there appears to be a bug in either `libgit2` or
//! `git2-rs` which prevents us from registering as a stateful transport:
//! apparently, the subtransport is instantiated twice, when it should only be
//! instantiated once, causing this assertion to fail:
//!
//! `libgit2/src/transports/smart.c:349: git_smart__negotiation_step: Assertion
//! `t->rpc || t->current_stream == stream' failed.`
//!
//! To work around this, we pretend to implement a stateless protocol by
//! indicating in the header line whether we want the remote side to only
//! advertise the refs, or wait for our haves. Of course, this makes this
//! transport incompatible with [`git-daemon`] for now, so the other side
//! needs to run our own [`GitServer`].
//!
//! [`git-daemon`]: https://git-scm.com/docs/git-daemon
//! [`GitServer`]: ../server/struct.GitServer.html

use std::{
    collections::HashMap,
    fmt::Display,
    io::{self, Read, Write},
    sync::{Arc, Once, RwLock},
};

use async_trait::async_trait;
use futures::{
    executor::block_on,
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
};
use git2::transport::{Service, SmartSubtransport, SmartSubtransportStream, Transport};
use tracing;

use crate::{git::url::GitUrl, hash::Hash, peer::PeerId};

type Factories = Arc<RwLock<HashMap<PeerId, Box<dyn GitStreamFactory>>>>;

lazy_static! {
    static ref FACTORIES: Factories = Arc::new(RwLock::new(HashMap::with_capacity(1)));
}

/// The underlying [`AsyncRead`] + [`AsyncRead`] of a [`RadSubTransport`]
///
/// We need this as a trait because we can't write `Box<dyn AsyncRead +
/// AsyncWrite + Unpin + Send>` directly.
pub trait GitStream: AsyncRead + AsyncWrite + Unpin + Send {}

/// Trait for types which can provide a [`GitStream`] over which we can send
/// / receive bytes to / from the specified peer.
#[async_trait]
pub trait GitStreamFactory: Sync + Send {
    async fn open_stream(&self, to: &PeerId) -> Option<Box<dyn GitStream>>;
}

/// Register the `rad+git://` transport with `libgit`.
///
/// # Safety:
///
/// The actual register call to `libgit` is guarded by [`Once`], it is thus safe
/// to call this function multiple times -- subsequent calls will return a new
/// [`RadTransport`], which can be used to register additional stream factories.
///
/// The first call to this function MUST, however, be externally synchronised
/// with all other calls to `libgit`.
pub fn register() -> RadTransport {
    static INIT: Once = Once::new();

    unsafe {
        INIT.call_once(|| {
            git2::transport::register("rad+git", move |remote| {
                Transport::smart(&remote, true, RadTransport::new())
            })
            .unwrap();
        })
    }

    RadTransport::new()
}

#[derive(Clone)]
pub struct RadTransport {
    fac: Factories,
}

impl RadTransport {
    fn new() -> Self {
        Self {
            fac: FACTORIES.clone(),
        }
    }

    /// Register an additional [`GitStreamFactory`], which can open git streams
    /// on behalf of `peer_id`.
    ///
    /// See the module documentation for why we key stream factories by sender.
    pub fn register_stream_factory(&self, peer_id: &PeerId, fac: Box<dyn GitStreamFactory>) {
        self.fac.write().unwrap().insert(peer_id.clone(), fac);
    }

    fn open_stream(&self, from: &PeerId, to: &PeerId) -> Option<Box<dyn GitStream>> {
        self.fac
            .read()
            .unwrap()
            .get(from)
            .and_then(|fac| block_on(fac.open_stream(to)))
    }
}

impl SmartSubtransport for RadTransport {
    fn action(
        &self,
        url: &str,
        action: Service,
    ) -> Result<Box<dyn SmartSubtransportStream>, git2::Error> {
        let url: GitUrl = url.parse().map_err(git_error)?;
        let stream = self
            .open_stream(&url.local_peer, &url.remote_peer)
            .ok_or_else(|| git_error(format!("No connection to {}", url.remote_peer)))?;

        Ok(Box::new(RadSubTransport {
            header_sent: false,
            remote_peer: url.remote_peer,
            remote_repo: url.repo,
            service: action,
            stream,
        }))
    }

    fn close(&self) -> Result<(), git2::Error> {
        Ok(())
    }
}

struct RadSubTransport {
    header_sent: bool,
    remote_peer: PeerId,
    remote_repo: Hash,
    service: Service,
    stream: Box<dyn GitStream>,
}

impl RadSubTransport {
    async fn ensure_header_sent(&mut self) -> io::Result<()> {
        if !self.header_sent {
            self.header_sent = true;
            self.stream
                .write_all(self.service_header().as_bytes())
                .await
        } else {
            Ok(())
        }
    }

    fn service_header(&self) -> String {
        match self.service {
            Service::UploadPackLs => format!(
                "git-upload-pack {}.git\0advertise\0host={}\0\n",
                self.remote_repo, self.remote_peer
            ),
            Service::UploadPack => format!(
                "git-upload-pack {}.git\0\0host={}\0\n",
                self.remote_repo, self.remote_peer
            ),
            Service::ReceivePackLs => format!(
                "git-receive-pack {}.git\0advertise\0host={}\0\n",
                self.remote_repo, self.remote_peer
            ),
            Service::ReceivePack => format!(
                "git-receive-pack {}.git\0\0host={}\0\n",
                self.remote_repo, self.remote_peer
            ),
        }
    }
}

impl Read for RadSubTransport {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        block_on(async {
            self.ensure_header_sent().await?;
            self.stream.read(buf).await.map_err(io_error)
        })
    }
}

impl Write for RadSubTransport {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        block_on(async {
            self.ensure_header_sent().await?;
            self.stream.write(buf).await.map_err(io_error)
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        block_on(async {
            self.ensure_header_sent().await?;
            self.stream.flush().await.map_err(io_error)
        })
    }
}

fn git_error<E: Display>(err: E) -> git2::Error {
    // libgit will always tell us "an unknown error occurred", so log them out
    // here
    tracing::error!("git transport error: {}", err);
    git2::Error::from_str(&err.to_string())
}

fn io_error<E: Display>(err: E) -> io::Error {
    io::Error::new(io::ErrorKind::Other, err.to_string())
}
