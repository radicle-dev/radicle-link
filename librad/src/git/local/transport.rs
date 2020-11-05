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
    collections::{HashMap, VecDeque},
    convert::TryFrom,
    fmt::Debug,
    io::{self, Cursor, Read, Write},
    panic::{self, UnwindSafe},
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio},
    sync::{Arc, Condvar, Mutex, Once, RwLock},
    thread,
    time::Duration,
};

use git2::transport::{Service, SmartSubtransport, SmartSubtransportStream, Transport};
use git_ext::{self as ext, into_git_err, RECEIVE_PACK_HEADER, UPLOAD_PACK_HEADER};
use thiserror::Error;

use super::{
    super::{
        identities,
        refs::{self, Refs},
        storage::{self, glob, Storage},
        types::namespace::Namespace,
        Urn,
    },
    url::LocalUrl,
};
use crate::{paths::Paths, peer::PeerId, signer::BoxedSigner};

lazy_static! {
    static ref SETTINGS: Arc<RwLock<HashMap<PeerId, SettingsInternal>>> =
        Arc::new(RwLock::new(HashMap::with_capacity(1)));
}

/// Register [`LocalTransport`] as a custom transport with `libgit2`.
///
/// This function should only be called once per program (it is however guarded
/// by a [`Once`], so repeated invocations are safe).
pub fn register() {
    static INIT: Once = Once::new();
    unsafe {
        INIT.call_once(move || {
            git2::transport::register(super::URL_SCHEME, move |remote| {
                Transport::smart(&remote, true, LocalTransportFactory::new())
            })
            .unwrap()
        });
    }
}

/// The settings for configuring a [`LocalTransport`] instance.
///
/// Note that transports are keyed by the public key of the `signer`, so this
/// can be used to configure different transports for different peers, e.g. in
/// tests.
#[derive(Clone)]
pub struct Settings {
    pub paths: Paths,
    pub signer: BoxedSigner,
}

/// Results of instantiations of [`LocalTransport`] as a `libgit2` transport.
///
/// See [`Self::wait`].
pub struct Results {
    done: Mutex<VecDeque<thread::Result<Result<(), Error>>>>,
    cvar: Condvar,
}

impl Results {
    /// Wait on the results of operations on [`git2::Remote`] using the
    /// [`LocalTransport`].
    ///
    /// This works around the issue of `libgit2` dropping the [`LocalTransport`]
    /// handle prematurely in some cases (e.g. `git-receive-pack` may
    /// trigger a `git gc`, but has sent main-band output already, so
    /// `libgit2` thinks it's done).
    ///
    /// [`Results`] is internally a queue -- results will appear in the order
    /// their corresponding child processes complete (i.e. **not**
    /// necessarily the order in which they were initated). Repeatedly
    /// calling [`Results::wait`] may thus yield more results.
    ///
    /// Normally, these results are not interesting, but users of
    /// [`LocalTransport`] as a custom `libgit2` transport **should** make
    /// sure to call [`Self::wait`] _at least_ before exiting the process,
    /// in order to ensure auxiliary operations complete. Doing so in a [`Drop`]
    /// impl would be a good choice.
    ///
    /// # Safety
    ///
    /// It is required to supply a timeout, in order to defend against the
    /// pathological case that the child process does not terminate, in
    /// which case we do not want to block the parent process. Note,
    /// however, that waiting on child processes is done by spawning threads --
    /// if an operation on [`git2::Remote`] is not [`Self::wait`]ed on, or the
    /// wait times out, the waiting thread will continue to run until the
    /// parent process is terminated.
    ///
    /// In other words, using [`LocalTransport`] as a custom `libgit2` transport
    /// may leak threads.
    ///
    /// # Errors
    ///
    /// If the `wait` timed out, [`None`] is returned, otherwise a
    /// [`thread::Result`]. If that is an [`Err`] value, the child process
    /// has panicked. Otherwise, the value contained in the [`Ok`] variant
    /// is the result of [`Connected::wait`].
    pub fn wait(&self, timeout: Duration) -> Option<Vec<thread::Result<Result<(), Error>>>> {
        let mut guard = self.done.lock().unwrap();
        loop {
            if guard.len() > 0 {
                return Some(guard.drain(0..).collect());
            } else {
                let res = self.cvar.wait_timeout(guard, timeout).unwrap();
                if res.1.timed_out() {
                    return None;
                } else {
                    guard = res.0;
                }
            }
        }
    }

    fn new() -> Self {
        Self {
            done: Mutex::new(VecDeque::new()),
            cvar: Condvar::new(),
        }
    }

    fn done(&self, res: thread::Result<Result<(), Error>>) {
        self.done.lock().unwrap().push_back(res);
        self.cvar.notify_all();
    }
}

struct SettingsInternal {
    settings: Settings,
    results: Arc<Results>,
}

#[derive(Clone)]
pub struct LocalTransportFactory {
    settings: Arc<RwLock<HashMap<PeerId, SettingsInternal>>>,
}

impl LocalTransportFactory {
    fn new() -> Self {
        Self {
            settings: SETTINGS.clone(),
        }
    }

    /// Set up the [`LocalTransportFactory`] with some [`Settings`], and obtain
    /// a side-channel to receive results of operations on [`git2::Remote`]
    /// using this transport.
    ///
    /// This must be called before any attempt to use the local transport. It
    /// **should** be called only once per program, and the returned
    /// [`Results`] awaited on in an appropriate place (e.g. a `Drop`
    /// finaliser).
    ///
    /// See [`Results::wait`] for more details.
    #[must_use = "results should be inspected"]
    pub fn configure(settings: Settings) -> Arc<Results> {
        let peer_id = settings.signer.peer_id();
        let results = Arc::new(Results::new());

        Self::new().settings.write().unwrap().insert(
            peer_id,
            SettingsInternal {
                settings,
                results: Arc::clone(&results),
            },
        );

        results
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

        match settings.get(&url.local_peer_id) {
            None => Err(git2::Error::from_str("local transport unconfigured")),
            Some(SettingsInternal { settings, results }) => {
                let mut transport = LocalTransport::new(settings.clone()).map_err(into_git_err)?;
                let mut child = transport
                    .connect(url, service, Mode::Stateless, Localio::piped())
                    .map_err(into_git_err)?;

                let stdin = child.process.stdin.take().unwrap();
                let stdout = child.process.stdout.take().unwrap();

                let results = Arc::clone(results);
                thread::spawn(move || {
                    let res = panic::catch_unwind(move || child.wait());
                    results.done(res)
                });

                let header = match service {
                    Service::UploadPackLs => Some(UPLOAD_PACK_HEADER.to_vec()),
                    Service::ReceivePackLs => Some(RECEIVE_PACK_HEADER.to_vec()),
                    _ => None,
                };

                Ok(Box::new(LocalStream {
                    read: LocalRead {
                        header,
                        inner: stdout,
                    },
                    write: stdin,
                }))
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
#[non_exhaustive]
pub enum Error {
    #[error("no such URN: {0}")]
    NoSuchUrn(Urn),

    #[error("no rad/self present and no default identity configured")]
    NoLocalIdentity,

    #[error("too many libgit2 transport streams")]
    StreamLimitExceeded,

    #[error(transparent)]
    Storage(#[from] storage::Error),

    #[error("child exited unsuccessfully")]
    Child(ExitStatus),

    #[error(transparent)]
    Refs(#[from] refs::stored::Error),

    #[error(transparent)]
    LocalId(#[from] identities::local::Error),

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
    on_success: Option<Box<dyn FnOnce() -> Result<(), Error> + Send + UnwindSafe + 'static>>,
}

impl Connected {
    #[allow(clippy::unit_arg)]
    #[tracing::instrument(skip(self), err)]
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
        let storage = Storage::open(&settings.paths, settings.signer)?;
        Ok(LocalTransport {
            storage: Arc::new(Mutex::new(storage)),
        })
    }

    #[tracing::instrument(level = "debug", skip(self, service, stdio), err)]
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
                &format!("--namespace={}", Namespace::from(&urn)),
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

        let on_success: Option<
            Box<dyn FnOnce() -> Result<(), Error> + Send + UnwindSafe + 'static>,
        > = match service {
            Service::ReceivePack => {
                let storage = self.storage.clone();
                let hook = move || {
                    let storage = storage.lock().unwrap();

                    // Update `rad/signed_refs`
                    Refs::update(&storage, &urn)?;

                    // Ensure we have a `rad/self`
                    let local_id = identities::local::load(&storage, urn.clone())
                        .transpose()
                        .or_else(|| identities::local::default(&storage).transpose())
                        .transpose()?;
                    match local_id {
                        None => Err(Error::NoLocalIdentity),
                        Some(local_id) => Ok(local_id.link(&storage, &urn)?),
                    }
                };

                Some(Box::new(hook))
            },

            _ => None,
        };

        Ok(Connected {
            process: child,
            on_success,
        })
    }

    fn guard_has_urn(&self, urn: &Urn) -> Result<(), Error> {
        let have = self
            .storage
            .lock()
            .unwrap()
            .has_urn(urn)
            .map_err(Error::from)?;
        if !have {
            Err(Error::NoSuchUrn(urn.clone()))
        } else {
            Ok(())
        }
    }

    fn visible_remotes(&self, urn: &Urn) -> Result<impl Iterator<Item = ext::RefLike>, Error> {
        let remotes = self
            .storage
            .lock()
            .unwrap()
            .references_glob(visible_remotes_glob(urn))?
            .filter_map(move |res| {
                res.map(|reference| {
                    reference
                        .name()
                        .and_then(|name| ext::RefLike::try_from(name).ok())
                })
                .transpose()
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(remotes.into_iter())
    }

    fn repo_path(&self) -> PathBuf {
        self.storage.lock().unwrap().path().to_path_buf()
    }
}

fn visible_remotes_glob(urn: &Urn) -> impl glob::Pattern + Debug {
    globset::Glob::new(&format!(
        "{}/*/{{heads,tags}}/*",
        reflike!("refs/namespaces")
            .join(Namespace::from(urn))
            .join(reflike!("refs/remotes"))
    ))
    .unwrap()
    .compile_matcher()
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

#[cfg(test)]
mod tests {
    use super::*;

    use crate::git::storage::glob::Pattern as _;

    #[test]
    fn visible_remotes_glob_seems_legit() {
        let urn = Urn::new(git2::Oid::zero().into());
        let glob = visible_remotes_glob(&urn);

        assert!(glob.matches(
            reflike!("refs/namespaces")
                .join(Namespace::from(&urn))
                .join(reflike!("refs/remotes/lolek/heads/next"))
        ));
        assert!(glob.matches(
            reflike!("refs/namespaces")
                .join(Namespace::from(&urn))
                .join(reflike!("refs/remotes/bolek/tags/v0.99"))
        ));
        assert!(!glob.matches("refs/heads/master"));
        assert!(!glob.matches("refs/namespaces/othernamespace/refs/remotes/tola/heads/next"));
        assert!(!glob.matches(
            reflike!("refs/namespaces")
                .join(Namespace::from(&urn))
                .join(reflike!("refs/heads/hidden"))
        ));
    }
}
