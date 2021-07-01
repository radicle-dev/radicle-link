// Copyright © 2019-2021 The Radicle Foundation <hello@radicle.foundation>
// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom, fmt::Debug, marker::PhantomData, path::Path};

use thiserror::Error;

use git2::string_array::StringArray;
use git_ext::{self as ext, blob, is_not_found_err, RefLike, RefspecPattern};
use std_ext::result::ResultExt as _;

use crate::{
    git::types::{reference, Many, One, Reference},
    identities::git::{Identities, Urn},
    paths::Paths,
    peer::PeerId,
};

use super::{
    config::{self, Config},
    glob::{self, Pattern},
};

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("malformed URN")]
    Ref(#[from] reference::FromUrnError),

    #[error(transparent)]
    Blob(#[from] ext::blob::Error),

    #[error(transparent)]
    Git(#[from] git2::Error),
}

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
    }
}

pub trait ReadOnlyStorage {
    fn has_urn(&self, urn: &Urn) -> Result<bool, Error>;

    fn has_ref(&self, reference: &Reference<One>) -> Result<bool, Error>;

    /// Check the existence of `oid` as a **commit**.
    ///
    /// The result will be `false` if:
    ///
    /// 1. No commit could be found for `oid`
    /// 2. The reference path for the `urn` could not be found (it defaults to
    /// `rad/id` if not provided)
    /// 3. The tip SHA was not in the history of the commit
    /// 4. The `oid` was the [`zero`][`git2::Oid::zero`] SHA.
    fn has_commit<Oid>(&self, urn: &Urn, oid: Oid) -> Result<bool, Error>
    where
        Oid: AsRef<git2::Oid> + Debug;

    /// Check the existence of `oid` as a **tag**.
    ///
    /// The result will be `false` if:
    ///
    /// 1. No tag could be found for `oid`
    /// 2. The reference path for the `urn` could not be found (it defaults to
    /// `rad/id` if not provided)
    /// 3. The SHA of the tag was not the same as the resolved reference
    /// 4. The `oid` was the [`zero`][`git2::Oid::zero`] SHA.
    fn has_tag<Oid>(&self, urn: &Urn, oid: Oid) -> Result<bool, Error>
    where
        Oid: AsRef<git2::Oid> + Debug;

    fn has_object<Oid>(&self, oid: Oid) -> Result<bool, Error>
    where
        Oid: AsRef<git2::Oid> + Debug;

    fn find_object<Oid>(&self, oid: Oid) -> Result<Option<git2::Object>, Error>
    where
        Oid: AsRef<git2::Oid> + Debug;

    fn tip(&self, urn: &Urn, kind: git2::ObjectType) -> Result<Option<git2::Object>, Error>;

    fn reference<'a>(
        &'a self,
        reference: &Reference<One>,
    ) -> Result<Option<git2::Reference<'a>>, Error>;

    fn references<'a>(&'a self, reference: &Reference<Many>) -> Result<References<'a>, Error>;

    fn reference_names<'a>(
        &'a self,
        reference: &Reference<Many>,
    ) -> Result<ReferenceNames<'a>, Error>;

    fn references_glob<'a, G: 'a>(&'a self, glob: G) -> Result<ReferencesGlob<'a, G>, Error>
    where
        G: Pattern + Debug;

    fn reference_names_glob<'a, G: 'a>(
        &'a self,
        glob: G,
    ) -> Result<ReferenceNamesGlob<'a, G>, Error>
    where
        G: Pattern + Debug;

    fn reference_oid(&self, reference: &Reference<One>) -> Result<ext::Oid, Error>;

    fn blob<'a>(
        &'a self,
        reference: &'a Reference<One>,
        path: &'a Path,
    ) -> Result<Option<git2::Blob<'a>>, Error>;

    fn remotes(&self) -> Result<StringArray, Error>;

    fn has_remote(&self, urn: &Urn, peer: PeerId) -> Result<bool, Error>;
}

/// Low-level operations on the link "monorepo".
pub struct ReadOnly {
    pub(super) backend: git2::Repository,
    pub(super) peer_id: PeerId,
}

impl ReadOnly {
    /// Open the [`ReadOnly`], which must exist.
    ///
    /// In contrast to a read-write [`super::Storage`], this does not require a
    /// `Signer`.
    ///
    /// # Concurrency
    ///
    /// [`ReadOnly`] can be sent between threads, but it can't be shared between
    /// threads. _Some_ operations are safe to perform concurrently in much
    /// the same way two `git` processes can access the same repository.
    /// However, if you need multiple [`ReadOnly`]s to be shared between
    /// threads, use a [`super::Pool`] instead.
    pub fn open(paths: &Paths) -> Result<Self, error::Init> {
        crate::git::init();
        let backend = git2::Repository::open(paths.git_dir())?;
        let peer_id = Config::try_from(&backend)?.peer_id()?;
        Ok(Self { backend, peer_id })
    }

    pub fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    pub fn path(&self) -> &Path {
        self.backend.path()
    }

    /// Check the existence of `oid` as a **commit**.
    ///
    /// The result will be `false` if:
    ///
    /// 1. No commit could be found for `oid`
    /// 2. The reference path for the `urn` could not be found (it defaults to
    /// `rad/id` if not provided)
    /// 3. The tip SHA was not in the history of the commit
    /// 4. The `oid` was the [`zero`][`git2::Oid::zero`] SHA.

    /// Check the existence of `oid` as a **tag**.
    ///
    /// The result will be `false` if:
    ///
    /// 1. No tag could be found for `oid`
    /// 2. The reference path for the `urn` could not be found (it defaults to
    /// `rad/id` if not provided)
    /// 3. The SHA of the tag was not the same as the resolved reference
    /// 4. The `oid` was the [`zero`][`git2::Oid::zero`] SHA.

    pub fn config(&self) -> Result<Config<PhantomData<!>>, Error> {
        Ok(Config::try_from(&self.backend)?)
    }

    pub(in crate::git) fn identities<'a, T: 'a>(&'a self) -> Identities<'a, T> {
        Identities::from(&self.backend)
    }
}

impl ReadOnlyStorage for ReadOnly {
    #[tracing::instrument(level = "debug", skip(self))]
    fn has_urn(&self, urn: &Urn) -> Result<bool, Error> {
        self.has_ref(&Reference::try_from(urn)?)
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn has_ref<'a>(&self, reference: &'a Reference<One>) -> Result<bool, Error> {
        self.backend
            .find_reference(RefLike::from(reference).as_str())
            .and(Ok(true))
            .or_matches(is_not_found_err, || Ok(false))
    }

    #[tracing::instrument(level = "debug", skip(self, urn), fields(urn = %urn))]
    fn has_commit<Oid>(&self, urn: &Urn, oid: Oid) -> Result<bool, Error>
    where
        Oid: AsRef<git2::Oid> + Debug,
    {
        let (oid, kind) = match self.find_object(oid)? {
            None => return Ok(false),
            Some(object) => match object.kind() {
                Some(git2::ObjectType::Commit) => (object.id(), git2::ObjectType::Commit),
                _ => return Ok(false),
            },
        };

        let tip = self.tip(urn, kind)?;
        Ok(tip
            .map(|tip| {
                Ok::<_, git2::Error>(
                    tip.id() == oid || self.backend.graph_descendant_of(tip.id(), oid)?,
                )
            })
            .transpose()?
            .unwrap_or(false))
    }

    #[tracing::instrument(level = "debug", skip(self, urn), fields(urn = %urn))]
    fn has_tag<Oid>(&self, urn: &Urn, oid: Oid) -> Result<bool, Error>
    where
        Oid: AsRef<git2::Oid> + Debug,
    {
        let (oid, kind) = match self.find_object(oid)? {
            None => return Ok(false),
            Some(object) => match object.kind() {
                Some(git2::ObjectType::Tag) => (object.id(), git2::ObjectType::Tag),
                _ => return Ok(false),
            },
        };

        let tip = self.tip(urn, kind)?;
        Ok(tip.map(|tip| tip.id() == oid).unwrap_or(false))
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn has_object<Oid>(&self, oid: Oid) -> Result<bool, Error>
    where
        Oid: AsRef<git2::Oid> + Debug,
    {
        let oid = oid.as_ref();
        if oid.is_zero() {
            // XXX: should this be a panic or error?
            tracing::warn!("zero oid");
            return Ok(false);
        }

        Ok(self.backend.odb()?.exists(*oid))
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn find_object<Oid>(&self, oid: Oid) -> Result<Option<git2::Object>, Error>
    where
        Oid: AsRef<git2::Oid> + Debug,
    {
        let oid = oid.as_ref();
        if oid.is_zero() {
            return Ok(None);
        }

        self.backend
            .find_object(*oid, None)
            .map(Some)
            .or_matches(is_not_found_err, || Ok(None))
    }

    #[tracing::instrument(level = "trace", skip(self))]
    fn tip(&self, urn: &Urn, kind: git2::ObjectType) -> Result<Option<git2::Object>, Error> {
        let reference = self
            .backend
            .find_reference(RefLike::from(&Reference::try_from(urn)?).as_str())
            .map(Some)
            .or_matches::<Error, _, _>(is_not_found_err, || Ok(None))?;

        match reference {
            None => Ok(None),
            Some(r) => r.peel(kind).map(Some).map_err(Error::from),
        }
    }

    #[tracing::instrument(level = "trace", skip(self))]
    fn reference<'a>(
        &'a self,
        reference: &Reference<One>,
    ) -> Result<Option<git2::Reference<'a>>, Error> {
        reference
            .find(&self.backend)
            .map(Some)
            .or_matches(is_not_found_err, || Ok(None))
    }

    #[tracing::instrument(level = "trace", skip(self))]
    fn references<'a>(&'a self, reference: &Reference<Many>) -> Result<References<'a>, Error> {
        self.references_glob(glob::RefspecMatcher::from(RefspecPattern::from(reference)))
            .map(|inner| References { inner })
    }

    #[tracing::instrument(level = "trace", skip(self))]
    fn reference_names<'a>(
        &'a self,
        reference: &Reference<Many>,
    ) -> Result<ReferenceNames<'a>, Error> {
        self.reference_names_glob(glob::RefspecMatcher::from(RefspecPattern::from(reference)))
            .map(|inner| ReferenceNames { inner })
    }

    #[tracing::instrument(level = "trace", skip(self))]
    fn references_glob<'a, G: 'a>(&'a self, glob: G) -> Result<ReferencesGlob<'a, G>, Error>
    where
        G: Pattern + Debug,
    {
        Ok(ReferencesGlob {
            iter: self.backend.references()?,
            glob,
        })
    }

    #[tracing::instrument(level = "trace", skip(self))]
    fn reference_names_glob<'a, G: 'a>(
        &'a self,
        glob: G,
    ) -> Result<ReferenceNamesGlob<'a, G>, Error>
    where
        G: Pattern + Debug,
    {
        Ok(ReferenceNamesGlob {
            iter: self.backend.references()?,
            glob,
        })
    }

    fn reference_oid(&self, reference: &Reference<One>) -> Result<ext::Oid, Error> {
        self.backend
            .refname_to_id(&reference.to_string())
            .map(ext::Oid::from)
            .map_err(Error::from)
    }

    #[tracing::instrument(level = "trace", skip(self))]
    fn blob<'a>(
        &'a self,
        reference: &'a Reference<One>,
        path: &'a Path,
    ) -> Result<Option<git2::Blob<'a>>, Error> {
        ext::Blob::Tip {
            branch: reference.into(),
            path,
        }
        .get(&self.backend)
        .map(Some)
        .or_matches(|e| matches!(e, blob::Error::NotFound(_)), || Ok(None))
    }

    fn remotes(&self) -> Result<StringArray, Error> {
        self.backend.remotes().map_err(Error::from)
    }

    fn has_remote(&self, urn: &Urn, peer: PeerId) -> Result<bool, Error> {
        let name = format!("{}/{}", urn.encode_id(), peer);
        self.backend
            .find_remote(&name)
            .and(Ok(true))
            .or_matches(is_not_found_err, || Ok(false))
    }
}

impl AsRef<ReadOnly> for ReadOnly {
    fn as_ref(&self) -> &Self {
        self
    }
}

pub struct ReferenceNames<'a> {
    inner: ReferenceNamesGlob<'a, glob::RefspecMatcher>,
}

impl<'a> Iterator for ReferenceNames<'a> {
    type Item = Result<ext::RefLike, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

pub struct ReferenceNamesGlob<'a, G: Pattern + Debug> {
    iter: git2::References<'a>,
    glob: G,
}

impl<'a, G> Iterator for ReferenceNamesGlob<'a, G>
where
    G: Pattern + Debug,
{
    type Item = Result<ext::RefLike, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let names = self.iter.names();
        for name in names {
            match name {
                Err(e) => return Some(Err(e.into())),
                Ok(name) => match ext::RefLike::try_from(name).ok() {
                    Some(refl) if self.glob.matches(Path::new(refl.as_str())) => {
                        return Some(Ok(refl))
                    },
                    _ => continue,
                },
            }
        }

        None
    }
}

pub struct References<'a> {
    inner: ReferencesGlob<'a, glob::RefspecMatcher>,
}

impl<'a> Iterator for References<'a> {
    type Item = Result<git2::Reference<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

pub struct ReferencesGlob<'a, G: Pattern + Debug> {
    iter: git2::References<'a>,
    glob: G,
}

impl<'a, G: Pattern + Debug> Iterator for ReferencesGlob<'a, G> {
    type Item = Result<git2::Reference<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        for reference in &mut self.iter {
            match reference {
                Ok(reference) => match reference.name() {
                    Some(name) if self.glob.matches(name) => return Some(Ok(reference)),
                    _ => continue,
                },

                Err(e) => return Some(Err(e.into())),
            }
        }
        None
    }
}
