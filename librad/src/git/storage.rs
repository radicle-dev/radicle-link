// Copyright © 2019-2021 The Radicle Foundation <hello@radicle.foundation>
// Copyright © 2021      The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom, fmt::Debug, marker::PhantomData, path::PathBuf};

use git_ext::is_not_found_err;
use thiserror::Error;

use crate::{
    paths::Paths,
    peer::PeerId,
    signer::{BoxedSigner, Signer, SomeSigner},
};

pub mod config;
pub mod fetcher;
pub mod glob;
pub mod pool;
pub mod read;
pub mod watch;

pub use config::Config;
pub use fetcher::{Fetcher, Fetchers};
pub use glob::Pattern;
pub use pool::{Pool, PoolError, Pooled, PooledRef};
pub use read::{Error, Storage as ReadOnly};
pub use watch::{NamespaceEvent, Watcher};

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum OpenError {
    #[error(transparent)]
    Config(#[from] config::Error),

    #[error(transparent)]
    Git(#[from] git2::Error),

    #[error("signer key does not match the key used at initialisation")]
    SignerKeyMismatch,
}

/// Low-level operations on the link "monorepo".
pub struct Storage {
    inner: read::Storage,
    signer: BoxedSigner,
    fetchers: Fetchers,
}

impl Storage {
    /// Open the [`Storage`], initialising it if it doesn't exist.
    ///
    /// Note that a [`Storage`] is tied to the [`Signer`] with which it was
    /// initialised, attempting to open it with a different one (that is, a
    /// different key) will return an error.
    ///
    /// # Concurrency
    ///
    /// [`Storage`] can be sent between threads, but it can't be shared between
    /// threads. _Some_ operations are safe to perform concurrently in much
    /// the same way two `git` processes can access the same repository.
    /// However, if you need multiple [`Storage`]s to be shared between
    /// threads, use a [`Pool`] instead.
    pub fn open<S>(paths: &Paths, signer: S) -> Result<Self, OpenError>
    where
        S: Signer + Clone,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        Self::with_fetchers(paths, signer, Default::default())
    }

    pub fn with_fetchers<S>(paths: &Paths, signer: S, fetchers: Fetchers) -> Result<Self, OpenError>
    where
        S: Signer + Clone,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        crate::git::init();

        let backend = match git2::Repository::open_bare(paths.git_dir()) {
            Err(e) if is_not_found_err(&e) => {
                let mut backend = git2::Repository::init_opts(
                    paths.git_dir(),
                    git2::RepositoryInitOptions::new()
                        .bare(true)
                        .no_reinit(true)
                        .external_template(false),
                )?;
                Config::init(&mut backend, &signer)?;

                Ok(backend)
            },
            Ok(repo) => Ok(repo),
            Err(e) => Err(e),
        }?;
        let peer_id = Config::try_from(&backend)?.peer_id()?;

        if peer_id != PeerId::from_signer(&signer) {
            return Err(OpenError::SignerKeyMismatch);
        }

        Ok(Self {
            inner: read::Storage { backend, peer_id },
            signer: BoxedSigner::from(SomeSigner { signer }),
            fetchers,
        })
    }

    /// Initialise a [`Storage`].
    ///
    /// If already initialised, this method does nothing. It is the same as
    /// `open`, but discarding the result.
    ///
    /// Use this if you need to ensure that an initialisation
    /// error is propagated promptly -- e.g. when you use a [`Pool`],
    /// initialisation would happen lazily, which makes it easy to miss
    /// errors.
    pub fn init<S>(paths: &Paths, signer: S) -> Result<(), OpenError>
    where
        S: Signer + Clone,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        Self::open(paths, signer)?;
        Ok(())
    }

    #[deprecated = "use `open` instead"]
    pub fn open_or_init<S>(paths: &Paths, signer: S) -> Result<Self, OpenError>
    where
        S: Signer + Clone,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        Self::open(paths, signer)
    }

    pub fn from_read_only<S>(ro: ReadOnly, signer: S, fetchers: Fetchers) -> Result<Self, OpenError>
    where
        S: Signer + Clone,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        if ro.peer_id != PeerId::from_signer(&signer) {
            return Err(OpenError::SignerKeyMismatch);
        }

        Ok(Self {
            inner: ro,
            signer: BoxedSigner::from(SomeSigner { signer }),
            fetchers,
        })
    }

    pub fn read_only(&self) -> &ReadOnly {
        &self.inner
    }

    pub fn peer_id(&self) -> &PeerId {
        self.inner.peer_id()
    }

    pub fn config(&self) -> Result<Config<BoxedSigner>, Error> {
        Ok(Config::try_from(self)?)
    }

    pub fn config_readonly(&self) -> Result<Config<PhantomData<!>>, Error> {
        Ok(Config::try_from(self.as_raw())?)
    }

    pub fn config_path(&self) -> PathBuf {
        config::path(self.as_raw())
    }

    pub fn watch(&self) -> watch::Watch {
        watch::Watch { storage: self }
    }

    pub(super) fn signer(&self) -> &BoxedSigner {
        &self.signer
    }

    // TODO: we would need to wrap a few more low-level git operations (such as:
    // create commit, manipulate refs, manipulate config) in order to be able to
    // model "capabilities" in terms of traits.
    pub(super) fn as_raw(&self) -> &git2::Repository {
        &self.backend
    }

    fn fetchers(&self) -> &Fetchers {
        &self.fetchers
    }
}

impl AsRef<Storage> for Storage {
    fn as_ref(&self) -> &Self {
        self
    }
}

impl AsRef<read::Storage> for Storage {
    fn as_ref(&self) -> &read::Storage {
        &self.inner
    }
}

impl std::ops::Deref for Storage {
    type Target = ReadOnly;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
