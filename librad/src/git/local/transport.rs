// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    convert::TryFrom,
    fmt::Debug,
    io,
    process::{Child, Command, ExitStatus, Stdio},
    sync::Arc,
};

use git2::transport::Service;
use git_ext::{self as ext};
use thiserror::Error;

use super::{
    super::{
        identities,
        refs::{self, Refs},
        storage::{self, glob, Storage},
        types::Namespace,
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

    #[error(transparent)]
    OpenStorage(#[from] OpenStorageError),

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

pub type OpenStorageError = Box<dyn std::error::Error + Send + Sync + 'static>;

pub trait CanOpenStorage: Send + Sync {
    fn open_storage(&self) -> Result<Box<dyn AsRef<Storage>>, OpenStorageError>;
}

pub(crate) fn with_local_transport<F, G, A>(
    open_storage: F,
    url: LocalUrl,
    g: G,
) -> Result<A, Error>
where
    F: CanOpenStorage + 'static,
    G: FnOnce(LocalUrl) -> A,
{
    internal::with(open_storage, url, g)
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
pub struct Settings {
    pub paths: Paths,
    pub signer: BoxedSigner,
}

impl CanOpenStorage for Settings {
    fn open_storage(&self) -> Result<Box<dyn AsRef<Storage>>, OpenStorageError> {
        let storage = Storage::open(&self.paths, self.signer.clone())?;
        Ok(Box::new(storage))
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Mode {
    Stateless,
    Stateful,
}

#[derive(Clone)]
pub struct LocalTransport {
    storage: Arc<Box<dyn CanOpenStorage>>,
}

impl From<Arc<Box<dyn CanOpenStorage>>> for LocalTransport {
    fn from(storage: Arc<Box<dyn CanOpenStorage>>) -> Self {
        Self { storage }
    }
}

impl From<Box<dyn CanOpenStorage>> for LocalTransport {
    fn from(storage: Box<dyn CanOpenStorage>) -> Self {
        Self {
            storage: Arc::new(storage),
        }
    }
}

impl LocalTransport {
    #[tracing::instrument(level = "debug", skip(self, service, stdio), err)]
    pub fn connect(
        &mut self,
        url: LocalUrl,
        service: Service,
        mode: Mode,
        stdio: Localio,
    ) -> Result<Connected, Error> {
        let _box = self.storage.open_storage()?;
        let storage = _box.as_ref();

        let urn = url.into();
        guard_has_urn(storage, &urn)?;

        let mut git = Command::new("git");
        git.envs(::std::env::vars().filter(|(key, _)| key.starts_with("GIT_TRACE")))
            .current_dir(storage.as_ref().path())
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
                visible_remotes(storage, &urn)?.for_each(|remote_ref| {
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

        let on_success: Option<Box<dyn FnOnce() -> Result<(), Error> + Send + 'static>> =
            match service {
                Service::ReceivePack => {
                    let storage = Arc::clone(&self.storage);
                    let hook = move || {
                        let _box = storage.open_storage()?;
                        let _dyn = _box.as_ref();
                        let storage = _dyn.as_ref();

                        // Update `rad/signed_refs`
                        Refs::update(storage, &urn)?;

                        // Ensure we have a `rad/self`
                        let local_id = identities::local::load(storage, urn.clone())
                            .transpose()
                            .or_else(|| identities::local::default(storage).transpose())
                            .transpose()?;
                        match local_id {
                            None => Err(Error::NoLocalIdentity),
                            Some(local_id) => Ok(local_id.link(storage, &urn)?),
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
}

fn guard_has_urn<S>(storage: S, urn: &Urn) -> Result<(), Error>
where
    S: AsRef<Storage>,
{
    let have = storage.as_ref().has_urn(urn).map_err(Error::from)?;
    if !have {
        Err(Error::NoSuchUrn(urn.clone()))
    } else {
        Ok(())
    }
}

fn visible_remotes<S>(storage: S, urn: &Urn) -> Result<impl Iterator<Item = ext::RefLike>, Error>
where
    S: AsRef<Storage>,
{
    let remotes = storage
        .as_ref()
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
