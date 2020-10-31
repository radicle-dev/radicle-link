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

use std::{convert::TryFrom, fmt::Debug, path::Path};

use git_ext::{self as ext, blob, is_not_found_err, RefLike, References, RefspecPattern};
use std_ext::result::ResultExt as _;
use thiserror::Error;

use super::types::{
    namespace::{AsNamespace, Namespace},
    reference,
    Many,
    NamespacedRef,
    One,
    Reference,
};
use crate::{identities::git::Identities, paths::Paths, peer::PeerId, signer::Signer};

pub mod config;
pub mod glob;
pub mod pool;

pub use config::Config;
pub use glob::Pattern;
pub use pool::{Pool, Pooled};

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
pub struct Storage<S> {
    backend: git2::Repository,
    peer_id: PeerId,
    signer: S,
}

impl<S> Storage<S>
where
    S: Signer,
{
    pub fn open(paths: &Paths, signer: S) -> Result<Self, Error> {
        let backend = git2::Repository::open_bare(paths.git_dir())?;
        let peer_id = Config::try_from(&backend)?.peer_id()?;

        if peer_id != PeerId::from_signer(&signer) {
            return Err(Error::SignerKeyMismatch);
        }

        Ok(Self {
            backend,
            peer_id,
            signer,
        })
    }

    pub fn init(paths: &Paths, signer: S) -> Result<Self, Error> {
        let mut backend = git2::Repository::init_opts(
            paths.git_dir(),
            git2::RepositoryInitOptions::new()
                .bare(true)
                .no_reinit(true)
                .external_template(false),
        )?;
        Config::init(&mut backend, &signer)?;
        let peer_id = PeerId::from_signer(&signer);

        Ok(Self {
            backend,
            peer_id,
            signer,
        })
    }

    pub fn open_or_init(paths: &Paths, signer: S) -> Result<Self, Error>
    where
        S: Clone,
    {
        let peer_id = PeerId::from_signer(&signer);
        match Self::open(paths, signer.clone()) {
            Err(Error::Git(e)) if is_not_found_err(&e) => Self::init(paths, signer),
            Err(e) => Err(e),
            Ok(this) if this.peer_id != peer_id => Err(Error::SignerKeyMismatch),

            Ok(this) => Ok(this),
        }
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
    pub fn has_ref<'a, N>(&self, reference: &'a NamespacedRef<N, One>) -> Result<bool, Error>
    where
        N: Debug,
        &'a N: AsNamespace,
    {
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
            return Ok(false);
        }

        match self.backend.find_commit(*oid) {
            Ok(commit) => {
                let namespace = Namespace::from(urn);
                let branch = {
                    let path = match &urn.path {
                        Some(refl) => refl.as_str(),
                        None => "rad/id",
                    };
                    path.strip_prefix("refs/").unwrap_or(path)
                };

                // FIXME: use references_glob
                let refs = References::from_globs(
                    &self.backend,
                    &[format!("refs/namespaces/{}/refs/{}", namespace, branch)],
                )?;

                for (_, oid) in refs.peeled() {
                    if oid == commit.id() || self.backend.graph_descendant_of(oid, commit.id())? {
                        return Ok(true);
                    }
                }

                Ok(false)
            },

            Err(e) if is_not_found_err(&e) => Ok(false),
            Err(e) => Err(e.into()),
        }
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
    pub fn reference<'a, N>(
        &'a self,
        reference: &NamespacedRef<N, One>,
    ) -> Result<Option<git2::Reference<'a>>, Error>
    where
        N: Debug,
        for<'b> &'b N: AsNamespace,
    {
        reference
            .find(&self.backend)
            .map(Some)
            .or_matches(is_not_found_err, || Ok(None))
    }

    #[tracing::instrument(level = "trace", skip(self), err)]
    pub fn references<'a, N>(
        &'a self,
        reference: &NamespacedRef<N, Many>,
    ) -> Result<impl Iterator<Item = Result<git2::Reference<'a>, Error>> + 'a, Error>
    where
        N: Debug,
        for<'b> &'b N: AsNamespace,
    {
        self.references_glob(glob::RefspecMatcher::from(RefspecPattern::from(reference)))
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
            Ok(reflike) if glob.matches(&reflike) => Some(Ok(reflike)),
            Ok(_) => None,

            Err(e) => Some(Err(e)),
        }))
    }

    #[tracing::instrument(level = "trace", skip(self), err)]
    pub fn blob<'a, N>(
        &'a self,
        reference: &'a NamespacedRef<N, One>,
        path: &'a Path,
    ) -> Result<Option<git2::Blob<'a>>, Error>
    where
        N: Debug,
        for<'b> &'b N: AsNamespace,
    {
        ext::Blob::Tip {
            branch: reference.into(),
            path,
        }
        .get(self.as_raw())
        .map(Some)
        .or_matches(|e| matches!(e, blob::Error::NotFound(_)), || Ok(None))
    }

    pub fn config(&self) -> Result<Config<S>, Error> {
        Ok(Config::try_from(self)?)
    }

    pub(super) fn signer(&self) -> &S {
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
}

struct ReferenceNames<'a> {
    iter: git2::References<'a>,
}

impl<'a> Iterator for ReferenceNames<'a> {
    type Item = Result<ext::RefLike, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut names = self.iter.names();
        while let Some(name) = names.next() {
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
