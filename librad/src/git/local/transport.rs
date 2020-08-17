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

use std::{
    collections::HashMap,
    io::{self, Cursor, Read, Write},
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio},
    sync::{Arc, Mutex, Once, RwLock},
    thread,
};

use git2::transport::{Service, SmartSubtransport, SmartSubtransportStream, Transport};
use thiserror::Error;

use crate::{
    git::{
        ext::{into_git_err, RECEIVE_PACK_HEADER, UPLOAD_PACK_HEADER},
        local::{self, url::LocalUrl},
        storage::{self, Storage},
    },
    paths::Paths,
    peer::PeerId,
    signer::BoxedSigner,
    uri::RadUrn,
};

lazy_static! {
    static ref SETTINGS: Arc<RwLock<HashMap<PeerId, Settings>>> =
        Arc::new(RwLock::new(HashMap::with_capacity(1)));
}

/// The settings for configuring a [`LocalTransportFactory`] and in turn a
/// [`LocalTransport`].
#[derive(Clone)]
pub struct Settings {
    pub paths: Paths,
    pub signer: BoxedSigner,
}

/// Register the local transport method to `git` so we can use our own custom
/// transport methods. See [`LocalTransportFactory`] and its `SmartSubtransport`
/// implementation for more details.
pub fn register(settings: Settings) {
    static INIT: Once = Once::new();

    LocalTransportFactory::new().configure(settings);
    unsafe {
        INIT.call_once(move || {
            git2::transport::register(local::URL_SCHEME, move |remote| {
                Transport::smart(&remote, true, LocalTransportFactory::new())
            })
            .unwrap()
        });
    }
}

#[derive(Clone)]
struct LocalTransportFactory {
    settings: Arc<RwLock<HashMap<PeerId, Settings>>>,
}

impl LocalTransportFactory {
    fn new() -> Self {
        Self {
            settings: SETTINGS.clone(),
        }
    }

    fn configure(&self, settings: Settings) {
        let peer_id = settings.signer.peer_id();
        self.settings.write().unwrap().insert(peer_id, settings);
    }
}

impl SmartSubtransport for LocalTransportFactory {
    fn action(
        &self,
        url: &str,
        service: Service,
    ) -> Result<Box<dyn SmartSubtransportStream>, git2::Error> {
        let settings = &*self.settings.read().unwrap();
        let url = url.parse::<LocalUrl>().map_err(into_git_err)?;

        match settings.get(url.peer_id()) {
            None => Err(git2::Error::from_str("local transport unconfigured")),
            Some(settings) => {
                let mut transport = LocalTransport::new(settings.clone()).map_err(into_git_err)?;
                let stream = transport
                    .stream(url, service, Localio::piped())
                    .map_err(into_git_err)?;

                Ok(Box::new(stream))
            },
        }
    }

    fn close(&self) -> Result<(), git2::Error> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Mode {
    Stateless,
    Stateful,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("No such URN: {0}")]
    NoSuchUrn(RadUrn),

    #[error(transparent)]
    Storage(#[from] storage::Error),

    #[error("Child exited unsuccessfully")]
    Child(ExitStatus),

    #[error(transparent)]
    Git(#[from] git2::Error),

    #[error(transparent)]
    Io(#[from] io::Error),
}

/// A running service (as per the [`Service`] argument) with it's stdio
/// connected as per [`Localio`].
///
/// [`Connected::wait`] MUST be called, in order to `wait(2)` on the child
/// process, and run post-service hooks.
#[must_use = "`wait` must be called"]
pub struct Connected {
    process: Child,
    on_success: Option<Box<dyn FnOnce() -> Result<(), Error> + Send + 'static>>,
}

impl Connected {
    pub fn wait(&mut self) -> Result<(), Error> {
        let status = self.process.wait()?;
        if status.success() {
            match self.on_success.take() {
                None => Ok(()),
                Some(f) => f(),
            }
        } else {
            Err(Error::Child(status))
        }
    }
}

// Belts + suspenders: we MUST ensure the child is a-`wait`-ed, to not leak
// zombies. However, `must_use` is only a warning lint by default, which the
// library user may choose to ignore. Hence, when `Connected` is dropped, we
// attempt to terminate the child iff it hasn't already, and then delegate to
// `Connected::wait` again.
//
// Note that we assume that `git` handles `SIGTERM` properly on unix platforms,
// however on Windows we are out of luck: there only really seems to exist
// `TerminateProcess`, which is equivalent to `SIGKILL`. Let's just hope for
// those poor Windows folks that `git-receive-pack` is fully `SIGKILL`-safe.
impl Drop for Connected {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Ok(None) = self.process.try_wait() {
            unsafe {
                libc::kill(self.process.id() as i32, libc::SIGTERM);
            }
        }

        // `SIGKILL` if the process is still running, so we don't block on `wait`.
        //
        // We can ignore the result -- it is either `InvalidInput` if the child
        // already exited, or there's nothing we can do anyway.
        if let Ok(None) = self.process.try_wait() {
            let _ = self.process.kill();
        }

        let _ = self.wait().map_err(|e| {
            // Make an effort to output an error in a disciplined way if we're
            // running with tracing enabled, or roguely if we're not (eg. the
            // remote helper won't).
            let msg = format!("Error waiting on child process: {}", e);
            let mut tracing_enabled = false;
            tracing::error!(
                traced = {
                    tracing_enabled = true;
                    tracing_enabled
                },
                "{}",
                msg
            );
            if !tracing_enabled {
                eprintln!("{}", msg)
            }
        });
    }
}

/// Connect the forked service's stdio to any [`Stdio`].
///
/// The service is either `git-upload-pack` or `git-receive-pack` depending on
/// the [`Service`] passed to [`LocalTransport::connect`].
pub struct Localio {
    pub child_stdin: Stdio,
    pub child_stdout: Stdio,
}

impl Localio {
    /// Arrange for pipes between parent and child.
    pub fn piped() -> Self {
        Self {
            child_stdin: Stdio::piped(),
            child_stdout: Stdio::piped(),
        }
    }

    /// Inherit stdio from the parent.
    pub fn inherit() -> Self {
        Self {
            child_stdin: Stdio::inherit(),
            child_stdout: Stdio::inherit(),
        }
    }
}

#[derive(Clone)]
pub struct LocalTransport {
    storage: Arc<Mutex<Storage<BoxedSigner>>>,
}

impl LocalTransport {
    pub fn new(settings: Settings) -> Result<Self, Error> {
        let storage = Storage::open(&settings.paths)?.with_signer(settings.signer)?;
        Ok(LocalTransport {
            storage: Arc::new(Mutex::new(storage)),
        })
    }

    pub fn stream(
        &mut self,
        url: LocalUrl,
        service: Service,
        stdio: Localio,
    ) -> Result<LocalStream, Error> {
        let mut child = self.connect(url, service, Mode::Stateless, stdio)?;

        let stdin = child.process.stdin.take().unwrap();
        let stdout = child.process.stdout.take().unwrap();

        // Spawn a thread to `wait(2)` on the child process.
        //
        // Note that we have no straightforward and portable way to forward
        // signals to the child process in Rust. When `LocalStream` is dropped,
        // `ChildStdin` and `ChildStdin` will close the underlying file
        // descriptors, which should cause `SIGPIPE` to be delivered to the
        // child. Unless `git` is somehow misbehaving, this should guarantee
        // that this thread is not leaked.
        thread::spawn(move || {
            child
                .wait()
                .map_err(|e| tracing::error!("Error waiting on child process: {}", e))
        });

        let header = match service {
            Service::UploadPackLs => Some(UPLOAD_PACK_HEADER.to_vec()),
            Service::ReceivePackLs => Some(RECEIVE_PACK_HEADER.to_vec()),
            _ => None,
        };

        Ok(LocalStream {
            read: LocalRead {
                header,
                inner: stdout,
            },
            write: stdin,
        })
    }

    pub fn connect(
        &mut self,
        url: LocalUrl,
        service: Service,
        mode: Mode,
        stdio: Localio,
    ) -> Result<Connected, Error> {
        let urn = url.into();
        self.guard_has_urn(&urn)?;

        let mut git = Command::new("git");
        git.envs(::std::env::vars().filter(|(key, _)| key.starts_with("GIT_TRACE")))
            .current_dir(self.repo_path())
            .args(&[
                &format!("--namespace={}", urn.id),
                "-c",
                "transfer.hiderefs=refs/",
                "-c",
                "transfer.hiderefs=!refs/heads",
                "-c",
                "transfer.hiderefs=!refs/tags",
            ]);

        match service {
            Service::UploadPack | Service::UploadPackLs => {
                // Fetching remotes is ok, pushing is not
                self.visible_remotes(&urn)?.for_each(|remote_ref| {
                    git.arg("-c")
                        .arg(format!("uploadpack.hiderefs=!^{}", remote_ref));
                });
                git.args(&["upload-pack", "--strict", "--timeout=5"]);
            },

            Service::ReceivePack | Service::ReceivePackLs => {
                git.arg("receive-pack");
            },
        }

        if let Mode::Stateless = mode {
            git.arg("--stateless-rpc");
        }

        if matches!(service, Service::UploadPackLs | Service::ReceivePackLs) {
            git.arg("--advertise-refs");
        }

        let Localio {
            child_stdin,
            child_stdout,
        } = stdio;

        let child = git
            .arg(".")
            .stdin(child_stdin)
            .stdout(child_stdout)
            .stderr(Stdio::inherit())
            .spawn()?;

        let this = self.clone();
        Ok(Connected {
            process: child,
            on_success: Some(Box::new(move || {
                if matches!(service, Service::ReceivePack) {
                    return this.update_refs(&urn);
                }

                Ok(())
            })),
        })
    }

    fn guard_has_urn(&self, urn: &RadUrn) -> Result<(), Error> {
        self.storage
            .lock()
            .unwrap()
            .has_urn(urn)
            .map_err(Error::from)
            .and_then(|have| {
                have.then_some(())
                    .ok_or_else(|| Error::NoSuchUrn(urn.clone()))
            })
    }

    fn visible_remotes(&self, urn: &RadUrn) -> Result<impl Iterator<Item = String>, Error> {
        const GLOBS: &[&str] = &["remotes/**/heads/*", "remotes/**/tags/*"];

        self.storage
            .lock()
            .unwrap()
            .references_glob(urn, GLOBS)
            .map(|iter| iter.map(|(name, _)| name).collect::<Vec<_>>())
            .map(|v| v.into_iter())
            .map_err(Error::from)
    }

    fn repo_path(&self) -> PathBuf {
        self.storage.lock().unwrap().path().to_path_buf()
    }

    fn update_refs(&self, urn: &RadUrn) -> Result<(), Error> {
        self.storage
            .lock()
            .unwrap()
            .update_refs(urn)
            .map_err(Error::from)
    }
}

pub struct LocalStream {
    read: LocalRead,
    write: ChildStdin,
}

impl Read for LocalStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read.read(buf)
    }
}

impl Write for LocalStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.write.flush()
    }
}

pub struct LocalRead {
    header: Option<Vec<u8>>,
    inner: ChildStdout,
}

impl Read for LocalRead {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.header.take() {
            None => self.inner.read(buf),
            Some(hdr) => Cursor::new(hdr).read(buf),
        }
    }
}
