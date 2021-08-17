// Copyright © 2019-2021 The Radicle Foundation <hello@radicle.foundation>
// Copyright © 2021      The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    convert::TryFrom,
    fmt::Debug,
    marker::PhantomData,
    path::{Path, PathBuf},
};

use crypto::{BoxedSigner, SomeSigner};
use git2::string_array::StringArray;
use git_ext::{self as ext, is_not_found_err};

use crate::{
    git::types::{Many, One, Reference},
    identities::git::Urn,
    paths::Paths,
    PeerId,
    Signer,
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
pub use read::{
    Error,
    ReadOnly,
    ReadOnlyStorage,
    ReferenceNames,
    ReferenceNamesGlob,
    References,
    ReferencesGlob,
};
pub use watch::{NamespaceEvent, Watcher};

pub mod error {
    use thiserror::Error;

    use super::config;

    #[derive(Debug, Error)]
    #[non_exhaustive]
    pub enum Init {
        #[error(transparent)]
        Config(#[from] config::Error),

        #[error(transparent)]
        Git(#[from] git2::Error),

        #[error("signer key does not match the key used at initialisation")]
        SignerKeyMismatch,
    }
}

/// Low-level operations on the link "monorepo".
pub struct Storage {
    inner: ReadOnly,
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
    pub fn open<S>(paths: &Paths, signer: S) -> Result<Self, error::Init>
    where
        S: Signer + Clone,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        Self::with_fetchers(paths, signer, Default::default())
    }

    pub fn with_fetchers<S>(
        paths: &Paths,
        signer: S,
        fetchers: Fetchers,
    ) -> Result<Self, error::Init>
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
            return Err(error::Init::SignerKeyMismatch);
        }

        Ok(Self {
            inner: ReadOnly { backend, peer_id },
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
    pub fn init<S>(paths: &Paths, signer: S) -> Result<(), error::Init>
    where
        S: Signer + Clone,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        Self::open(paths, signer)?;
        Ok(())
    }

    #[deprecated = "use `open` instead"]
    pub fn open_or_init<S>(paths: &Paths, signer: S) -> Result<Self, error::Init>
    where
        S: Signer + Clone,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        Self::open(paths, signer)
    }

    pub fn from_read_only<S>(
        ro: ReadOnly,
        signer: S,
        fetchers: Fetchers,
    ) -> Result<Self, error::Init>
    where
        S: Signer + Clone,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        if ro.peer_id != PeerId::from_signer(&signer) {
            return Err(error::Init::SignerKeyMismatch);
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

    pub fn path(&self) -> &Path {
        self.inner.path()
    }

    pub fn config(&self) -> Result<Config<BoxedSigner>, config::Error> {
        Config::try_from(self)
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
        &self.inner.backend
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

impl AsRef<ReadOnly> for Storage {
    fn as_ref(&self) -> &ReadOnly {
        &self.inner
    }
}

impl ReadOnlyStorage for Storage {
    fn has_urn(&self, urn: &Urn) -> Result<bool, Error> {
        self.inner.has_urn(urn)
    }

    fn has_ref(&self, reference: &Reference<One>) -> Result<bool, Error> {
        self.inner.has_ref(reference)
    }

    fn has_commit<Oid>(&self, urn: &Urn, oid: Oid) -> Result<bool, Error>
    where
        Oid: AsRef<git2::Oid> + Debug,
    {
        self.inner.has_commit(urn, oid)
    }

    fn has_tag<Oid>(&self, urn: &Urn, oid: Oid) -> Result<bool, Error>
    where
        Oid: AsRef<git2::Oid> + Debug,
    {
        self.inner.has_tag(urn, oid)
    }

    fn has_object<Oid>(&self, oid: Oid) -> Result<bool, Error>
    where
        Oid: AsRef<git2::Oid> + Debug,
    {
        self.inner.has_object(oid)
    }

    fn find_object<Oid>(&self, oid: Oid) -> Result<Option<git2::Object>, Error>
    where
        Oid: AsRef<git2::Oid> + Debug,
    {
        self.inner.find_object(oid)
    }

    fn tip(&self, urn: &Urn, kind: git2::ObjectType) -> Result<Option<git2::Object>, Error> {
        self.inner.tip(urn, kind)
    }

    fn reference<'a>(
        &'a self,
        reference: &Reference<One>,
    ) -> Result<Option<git2::Reference<'a>>, Error> {
        self.inner.reference(reference)
    }

    fn references<'a>(&'a self, reference: &Reference<Many>) -> Result<References<'a>, Error> {
        self.inner.references(reference)
    }

    fn reference_names<'a>(
        &'a self,
        reference: &Reference<Many>,
    ) -> Result<ReferenceNames<'a>, Error> {
        self.inner.reference_names(reference)
    }

    fn references_glob<'a, G: 'a>(&'a self, glob: G) -> Result<ReferencesGlob<'a, G>, Error>
    where
        G: Pattern + Debug,
    {
        self.inner.references_glob(glob)
    }

    fn reference_names_glob<'a, G: 'a>(
        &'a self,
        glob: G,
    ) -> Result<ReferenceNamesGlob<'a, G>, Error>
    where
        G: Pattern + Debug,
    {
        self.inner.reference_names_glob(glob)
    }

    fn reference_oid(&self, reference: &Reference<One>) -> Result<ext::Oid, Error> {
        self.inner.reference_oid(reference)
    }

    fn blob<'a>(
        &'a self,
        reference: &'a Reference<One>,
        path: &'a Path,
    ) -> Result<Option<git2::Blob<'a>>, Error> {
        self.inner.blob(reference, path)
    }

    fn remotes(&self) -> Result<StringArray, Error> {
        self.inner.remotes()
    }

    fn has_remote(&self, urn: &Urn, peer: PeerId) -> Result<bool, Error> {
        self.inner.has_remote(urn, peer)
    }
}
