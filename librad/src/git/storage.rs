// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom, fmt::Debug, path::Path};

use git_ext::{self as ext, blob, is_not_found_err, RefLike, RefspecPattern};
use std_ext::result::ResultExt as _;
use thiserror::Error;

use super::types::{reference, Many, Namespace, One, Reference};
use crate::{
    identities::git::Identities,
    internal::klock::Klock,
    paths::Paths,
    peer::PeerId,
    signer::{BoxedSigner, Signer, SomeSigner},
};

pub mod config;
pub mod glob;
pub mod pool;

pub use config::Config;
pub use glob::Pattern;
pub use pool::{Pool, PoolError, Pooled, PooledRef};

// FIXME: should be at the crate root
pub use crate::identities::git::Urn;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("signer key does not match the key used at initialisation")]
    SignerKeyMismatch,

    #[error("malformed URN")]
    Ref(#[from] reference::FromUrnError),

    #[error(transparent)]
    Config(#[from] config::Error),

    #[error(transparent)]
    Blob(#[from] ext::blob::Error),

    #[error(transparent)]
    Git(#[from] git2::Error),
}

/// Low-level operations on the link "monorepo".
pub struct Storage {
    backend: git2::Repository,
    peer_id: PeerId,
    signer: BoxedSigner,
    fetch_lock: Klock<Urn>,
}

impl Storage {
    pub fn open<S>(paths: &Paths, signer: S) -> Result<Self, Error>
    where
        S: Signer + Clone,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        Self::with_fetch_lock(paths, signer, Default::default())
    }

    fn with_fetch_lock<S>(paths: &Paths, signer: S, fetch_lock: Klock<Urn>) -> Result<Self, Error>
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
            return Err(Error::SignerKeyMismatch);
        }

        Ok(Self {
            backend,
            peer_id,
            signer: BoxedSigner::from(SomeSigner { signer }),
            fetch_lock,
        })
    }

    pub fn init<S>(paths: &Paths, signer: S) -> Result<(), Error>
    where
        S: Signer + Clone,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        Self::open(paths, signer)?;
        Ok(())
    }

    #[deprecated = "use `open` instead"]
    pub fn open_or_init<S>(paths: &Paths, signer: S) -> Result<Self, Error>
    where
        S: Signer + Clone,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        Self::open(paths, signer)
    }

    pub fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    pub fn path(&self) -> &Path {
        &self.backend.path()
    }

    #[tracing::instrument(level = "debug", skip(self), err)]
    pub fn has_urn(&self, urn: &Urn) -> Result<bool, Error> {
        self.has_ref(&Reference::try_from(urn)?)
    }

    #[tracing::instrument(level = "debug", skip(self), err)]
    pub fn has_ref<'a>(&self, reference: &'a Reference<One>) -> Result<bool, Error> {
        self.backend
            .find_reference(RefLike::from(reference).as_str())
            .and(Ok(true))
            .or_matches(is_not_found_err, || Ok(false))
    }

    #[tracing::instrument(level = "debug", skip(self), err)]
    pub fn has_commit<Oid>(&self, urn: &Urn, oid: Oid) -> Result<bool, Error>
    where
        Oid: AsRef<git2::Oid> + Debug,
    {
        let oid = oid.as_ref();
        if oid.is_zero() {
            // XXX: should this be a panic or error?
            tracing::warn!("zero oid");
            return Ok(false);
        }

        self.backend
            .find_commit(*oid)
            .and_then(|commit| {
                let namespace = Namespace::from(urn);
                let branch = {
                    let path = match &urn.path {
                        Some(refl) => refl.as_str(),
                        None => "rad/id",
                    };
                    path.strip_prefix("refs/").unwrap_or(path)
                };
                self.backend
                    .refname_to_id(&format!("refs/namespaces/{}/refs/{}", namespace, branch))
                    .and_then(|tip| {
                        Ok(tip == commit.id()
                            || self.backend.graph_descendant_of(tip, commit.id())?)
                    })
                    .or_matches(is_not_found_err, || Ok(false))
            })
            .or_matches(is_not_found_err, || Ok(false))
    }

    #[tracing::instrument(level = "debug", skip(self), err)]
    pub fn has_object<Oid>(&self, oid: Oid) -> Result<bool, Error>
    where
        Oid: AsRef<git2::Oid> + Debug,
    {
        let oid = oid.as_ref();
        if oid.is_zero() {
            return Ok(false);
        }

        Ok(self.backend.odb()?.exists(*oid))
    }

    #[tracing::instrument(level = "trace", skip(self), err)]
    pub fn tip(&self, urn: &Urn) -> Result<Option<ext::Oid>, Error> {
        let reference = self
            .backend
            .find_reference(RefLike::from(&Reference::try_from(urn)?).as_str())
            .map(Some)
            .or_matches::<Error, _, _>(is_not_found_err, || Ok(None))?;

        match reference {
            None => Ok(None),
            Some(r) => Ok(Some(r.peel_to_commit()?.id().into())),
        }
    }

    #[tracing::instrument(level = "trace", skip(self), err)]
    pub fn reference<'a>(
        &'a self,
        reference: &Reference<One>,
    ) -> Result<Option<git2::Reference<'a>>, Error> {
        reference
            .find(&self.backend)
            .map(Some)
            .or_matches(is_not_found_err, || Ok(None))
    }

    #[tracing::instrument(level = "trace", skip(self), err)]
    pub fn references<'a>(
        &'a self,
        reference: &Reference<Many>,
    ) -> Result<impl Iterator<Item = Result<git2::Reference<'a>, Error>> + 'a, Error> {
        self.references_glob(glob::RefspecMatcher::from(RefspecPattern::from(reference)))
    }

    #[tracing::instrument(level = "trace", skip(self), err)]
    pub fn reference_names<'a>(
        &'a self,
        reference: &Reference<Many>,
    ) -> Result<impl Iterator<Item = Result<ext::RefLike, Error>> + 'a, Error> {
        self.reference_names_glob(glob::RefspecMatcher::from(RefspecPattern::from(reference)))
    }

    #[tracing::instrument(level = "trace", skip(self), err)]
    pub fn references_glob<'a, G: 'a>(
        &'a self,
        glob: G,
    ) -> Result<impl Iterator<Item = Result<git2::Reference<'a>, Error>> + 'a, Error>
    where
        G: Pattern + Debug,
    {
        Ok(self
            .backend
            .references()?
            .filter_map(move |reference| match reference {
                Ok(reference) => match reference.name() {
                    Some(name) if glob.matches(name) => Some(Ok(reference)),
                    _ => None,
                },

                Err(e) => Some(Err(e.into())),
            }))
    }

    #[tracing::instrument(level = "trace", skip(self), err)]
    pub fn reference_names_glob<'a, G: 'a>(
        &'a self,
        glob: G,
    ) -> Result<impl Iterator<Item = Result<ext::RefLike, Error>> + 'a, Error>
    where
        G: Pattern + Debug,
    {
        let iter = ReferenceNames {
            iter: self.backend.references()?,
        };
        Ok(iter.filter_map(move |refname| match refname {
            Ok(reflike) if glob.matches(Path::new(reflike.as_str())) => Some(Ok(reflike)),
            Ok(_) => None,

            Err(e) => Some(Err(e)),
        }))
    }

    #[tracing::instrument(level = "trace", skip(self), err)]
    pub fn blob<'a>(
        &'a self,
        reference: &'a Reference<One>,
        path: &'a Path,
    ) -> Result<Option<git2::Blob<'a>>, Error> {
        ext::Blob::Tip {
            branch: reference.into(),
            path,
        }
        .get(self.as_raw())
        .map(Some)
        .or_matches(|e| matches!(e, blob::Error::NotFound(_)), || Ok(None))
    }

    pub fn config(&self) -> Result<Config<BoxedSigner>, Error> {
        Ok(Config::try_from(self)?)
    }

    pub(super) fn signer(&self) -> &BoxedSigner {
        &self.signer
    }

    pub(super) fn identities<'a, T: 'a>(&'a self) -> Identities<'a, T> {
        Identities::from(self.as_raw())
    }

    // TODO: we would need to wrap a few more low-level git operations (such as:
    // create commit, manipulate refs, manipulate config) in order to be able to
    // model "capabilities" in terms of traits.
    pub(super) fn as_raw(&self) -> &git2::Repository {
        &self.backend
    }

    pub(super) fn fetch_lock(&self) -> &Klock<Urn> {
        &self.fetch_lock
    }
}

impl AsRef<Storage> for Storage {
    fn as_ref(&self) -> &Self {
        self
    }
}

struct ReferenceNames<'a> {
    iter: git2::References<'a>,
}

impl<'a> Iterator for ReferenceNames<'a> {
    type Item = Result<ext::RefLike, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let names = self.iter.names();
        for name in names {
            match name {
                Err(e) => return Some(Err(e.into())),
                Ok(name) => match ext::RefLike::try_from(name).ok() {
                    Some(refl) => return Some(Ok(refl)),
                    None => continue,
                },
            }
        }

        None
    }
}
