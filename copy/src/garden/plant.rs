// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{io, marker::PhantomData, path::PathBuf};

use serde::{Deserialize, Serialize};

use librad::{
    git::local::{transport::CanOpenStorage, url::LocalUrl},
    git_ext::{self, OneLevel},
    identities::payload,
    internal::canonical::Cstring,
};

use super::{AsPayload, CreateRepo};
use crate::{git, sealed::Sealed};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("a directory at `{0}` already exists")]
    AlreadExists(PathBuf),

    #[error(transparent)]
    Git(#[from] git::Error),

    #[error(transparent)]
    Ref(#[from] git_ext::name::Error),

    #[error(transparent)]
    Io(#[from] io::Error),
}

/// For construction, use [`Plant::new`] followed by [`Plant::validate`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plant<V> {
    payload: payload::Project,
    path: PathBuf,
    valid: V,
}

impl<V> Sealed for Plant<V> {}

impl<V> AsPayload for Plant<V> {
    fn as_payload(&self) -> payload::Project {
        self.payload.clone()
    }
}

impl CreateRepo for Plant<Valid> {
    type Error = Error;

    fn init<F>(self, url: LocalUrl, transport: F) -> Result<git2::Repository, Self::Error>
    where
        F: CanOpenStorage + Clone + 'static,
    {
        self.init(url, transport)
    }
}

impl<V> Plant<V> {
    pub fn path(&self) -> PathBuf {
        self.path.join(self.payload.name.to_string())
    }
}

pub type Invalid = PhantomData<!>;
pub type Valid = PhantomData<!>;

impl Plant<Invalid> {
    pub fn new(
        description: Option<Cstring>,
        default_branch: OneLevel,
        name: Cstring,
        path: PathBuf,
    ) -> Self {
        // FIXME: actually pass the payload
        Self {
            payload: payload::Project {
                description,
                default_branch: Some(default_branch.as_str().into()),
                name,
            },
            path,
            valid: PhantomData,
        }
    }
}

impl Plant<Valid> {
    pub fn validate(invalid: Plant<Invalid>) -> Result<Self, Error> {
        let repo_path = invalid.path();

        if repo_path.is_file() {
            return Err(Error::AlreadExists(repo_path));
        }

        if repo_path.exists() && repo_path.is_dir() && repo_path.read_dir()?.next().is_some() {
            return Err(Error::AlreadExists(repo_path));
        }

        Ok(Self {
            payload: invalid.payload,
            path: invalid.path,
            valid: PhantomData,
        })
    }

    pub fn init<F>(self, url: LocalUrl, open_storage: F) -> Result<git2::Repository, Error>
    where
        F: CanOpenStorage + Clone + 'static,
    {
        let path = self.path();
        let default_branch = git::determine_default_branch(&self.payload)?;

        tracing::debug!(path = %path.display(), branch = %default_branch, "setting up new repository");

        let repo: git2::Repository = git::init(path, &self.payload.description, &default_branch)?;
        git::initial_commit(
            &repo,
            &default_branch,
            &git2::Signature::now("Radicle Automated", "Radicle Automated")
                .map_err(git::Error::Git)?,
        )?;
        let remote = git::setup_remote(&repo, open_storage, url, &default_branch)?;
        git::set_upstream(&repo, &remote, default_branch)?;

        Ok(repo)
    }
}
