// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{io, marker::PhantomData, path::PathBuf};

use serde::{Deserialize, Serialize};

use librad::{
    git::local::{transport::CanOpenStorage, url::LocalUrl},
    git_ext::OneLevel,
    identities::payload,
    internal::canonical::Cstring,
};

use crate::{git, AsPayload, CreateRepo};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("a directory at `{0}` already exists")]
    AlreadExists(PathBuf),

    #[error(transparent)]
    Git(#[from] git::Error),

    #[error(transparent)]
    Io(#[from] io::Error),
}

/// For construction, use [`New::new`] followed by [`New::validate`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct New<V> {
    description: Option<Cstring>,
    default_branch: OneLevel,
    name: Cstring,
    path: PathBuf,
    valid: V,
}

impl<V> AsPayload for New<V> {
    fn as_payload(&self) -> payload::Project {
        payload::Project {
            default_branch: Some(Cstring::from(self.default_branch.as_str())),
            description: self.description.clone(),
            name: self.name.clone(),
        }
    }
}

impl CreateRepo for New<Valid> {
    type Error = Error;

    fn init<F>(self, url: LocalUrl, transport: F) -> Result<git2::Repository, Self::Error>
    where
        F: CanOpenStorage + 'static,
    {
        self.init(url, transport)
    }
}

impl<V> New<V> {
    pub fn path(&self) -> PathBuf {
        self.path.join(self.name.to_string())
    }
}

pub type Invalid = PhantomData<!>;
pub type Valid = PhantomData<!>;

impl New<Invalid> {
    pub fn new(
        description: Option<Cstring>,
        default_branch: OneLevel,
        name: Cstring,
        path: PathBuf,
    ) -> Self {
        Self {
            description,
            default_branch,
            name,
            path,
            valid: PhantomData,
        }
    }
}

impl New<Valid> {
    pub fn validate(invalid: New<Invalid>) -> Result<Self, Error> {
        let repo_path = invalid.path();

        if repo_path.is_file() {
            return Err(Error::AlreadExists(repo_path));
        }

        if repo_path.exists() && repo_path.is_dir() && repo_path.read_dir()?.next().is_some() {
            return Err(Error::AlreadExists(repo_path));
        }

        Ok(Self {
            description: invalid.description,
            default_branch: invalid.default_branch,
            name: invalid.name,
            path: invalid.path,
            valid: PhantomData,
        })
    }

    pub fn init<F>(self, url: LocalUrl, open_storage: F) -> Result<git2::Repository, Error>
    where
        F: CanOpenStorage + 'static,
    {
        let path = self.path();
        tracing::debug!("Setting up new repository @ '{}'", path.display());
        let repo = git::init(path, &self.description, &self.default_branch)?;
        git::initial_commit(
            &repo,
            &self.default_branch,
            &git2::Signature::now("Radicle Automated", "Radicle Automated")
                .map_err(git::Error::Git)?,
        )?;
        let remote = git::setup_remote(&repo, open_storage, url, &self.default_branch)?;
        git::set_upstream(&repo, &remote, self.default_branch)?;

        Ok(repo)
    }
}
