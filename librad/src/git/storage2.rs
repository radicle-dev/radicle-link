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

use std::convert::TryFrom;

use thiserror::Error;

use crate::{
    git::{
        ext::{self, is_not_found_err},
        types::{Multiple, Namespace2, Reference, Single},
    },
    identities,
    internal::result::ResultExt as _,
    keys,
    paths::Paths,
    peer::PeerId,
    signer::Signer,
};

mod config;

use config::Config;

// FIXME: should be at the crate root
pub type Urn = identities::git::Urn;

#[derive(Debug, Error)]
pub enum Error {
    #[error("signer key does not match the key used at initialisation")]
    SignerKeyMismatch,

    #[error(transparent)]
    Config(#[from] config::Error),

    #[error(transparent)]
    Git(#[from] git2::Error),
}

pub struct Storage {
    backend: git2::Repository,
    peer_id: PeerId,
}

impl Storage {
    pub fn open(paths: &Paths) -> Result<Self, Error> {
        let backend = git2::Repository::open_bare(paths.git_dir())?;
        let peer_id = Config::try_from(&backend)?.peer_id()?;

        Ok(Self { backend, peer_id })
    }

    pub fn init<S>(paths: &Paths, signer: &S) -> Result<Self, Error>
    where
        S: Signer,
        S::Error: keys::SignError,
    {
        let mut backend = git2::Repository::init_opts(
            paths.git_dir(),
            git2::RepositoryInitOptions::new()
                .bare(true)
                .no_reinit(true)
                .external_template(false),
        )?;
        Config::init(&mut backend, signer, None)?;
        let peer_id = PeerId::from_signer(signer);

        Ok(Self { backend, peer_id })
    }

    pub fn open_or_init<S>(paths: &Paths, signer: &S) -> Result<Self, Error>
    where
        S: Signer,
        S::Error: keys::SignError,
    {
        let peer_id = PeerId::from_signer(signer);
        match Self::open(paths) {
            Err(Error::Git(e)) if is_not_found_err(&e) => Self::init(paths, signer),
            Err(e) => Err(e),
            Ok(this) if this.peer_id != peer_id => Err(Error::SignerKeyMismatch),

            Ok(this) => Ok(this),
        }
    }

    pub fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    pub fn has_urn(&self, urn: &Urn) -> Result<bool, Error> {
        let namespace = Namespace2::from(urn);
        let branch = {
            let path = match &urn.path {
                Some(refl) => refl.as_str(),
                None => "rad/id",
            };
            path.strip_prefix("refs/").unwrap_or(path)
        };

        // TODO: implement in terms of `self::has_ref` -- can't construct a
        // `Reference` from here at the moment
        self.backend
            .find_reference(&format!("refs/namespaces/{}/refs/{}", namespace, branch))
            .map(|_| true)
            .or_matches(is_not_found_err, || Ok(false))
    }

    pub fn has_ref<T>(&self, reference: &Reference<Namespace2, PeerId, T>) -> Result<bool, Error> {
        let found = self
            .backend
            .references_glob(&reference.to_string())?
            .names()
            .filter_map(Result::ok)
            .count();
        Ok(found > 0)
    }

    pub fn has_commit<Oid>(&self, urn: &Urn, oid: Oid) -> Result<bool, Error>
    where
        Oid: AsRef<git2::Oid>,
    {
        let oid = oid.as_ref();
        if oid.is_zero() {
            return Ok(false);
        }

        match self.backend.find_commit(*oid) {
            Ok(commit) => {
                let namespace = Namespace2::from(urn);
                let branch = {
                    let path = match &urn.path {
                        Some(refl) => refl.as_str(),
                        None => "rad/id",
                    };
                    path.strip_prefix("refs/").unwrap_or(path)
                };

                let refs = ext::References::from_globs(
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

    pub fn has_object<Oid>(&self, oid: Oid) -> Result<bool, Error>
    where
        Oid: AsRef<git2::Oid>,
    {
        let oid = oid.as_ref();
        if oid.is_zero() {
            return Ok(false);
        }

        Ok(self.backend.odb()?.exists(*oid))
    }

    pub fn reference<'a>(
        &'a self,
        reference: &Reference<Namespace2, PeerId, Single>,
    ) -> Result<git2::Reference<'a>, Error> {
        Ok(reference.find(&self.backend)?)
    }

    pub fn references<'a>(
        &'a self,
        reference: &Reference<Namespace2, PeerId, Multiple>,
    ) -> Result<ext::References<'a>, Error> {
        Ok(reference.references(&self.backend)?)
    }
}
