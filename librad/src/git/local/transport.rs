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
    convert::TryFrom,
    fmt::Debug,
    io::{self, Cursor, Read, Write},
    panic::{self, UnwindSafe},
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};

use git2::transport::Service;
use git_ext::{self as ext};
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
use crate::{paths::Paths, signer::BoxedSigner};

mod internal;

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

pub fn with_local_transport<F, G, A>(
    open_storage: F,
    repo: &git2::Repository,
    url: impl AsRef<LocalUrl>,
    timeout: Duration,
    g: G,
) -> Result<A, Error>
where
    F: Fn() -> Result<Storage<BoxedSigner>, Box<dyn std::error::Error + Send + Sync + 'static>>
        + Send
        + Sync
        + 'static,
    G: FnOnce(&mut git2::Remote) -> A,
{
    let (url, results) = internal::activate(open_storage, url.as_ref().clone());
    let mut remote = repo.remote_anonymous(&url.to_string())?;
    let ret = g(&mut remote);

    match results.wait(timeout) {
        None => panic!("a subprocess failed to terminate"),
        Some(ress) => {
            for res in ress {
                match res {
                    Err(e) => panic::resume_unwind(e),
                    Ok(Err(inner)) => return Err(inner),
                    Ok(Ok(())) => (),
                }
            }
        },
    }

    Ok(ret)
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

pub struct Settings {
    pub paths: Paths,
    pub signer: BoxedSigner,
}

#[derive(Clone, Copy, Debug)]
pub enum Mode {
    Stateless,
    Stateful,
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

impl From<Storage<BoxedSigner>> for LocalTransport {
    fn from(storage: Storage<BoxedSigner>) -> Self {
        Self {
            storage: Arc::new(Mutex::new(storage)),
        }
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
