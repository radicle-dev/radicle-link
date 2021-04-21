// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{marker::PhantomData, path::PathBuf};

use serde::{Deserialize, Serialize};

use librad::{
    git::local::{transport::CanOpenStorage, url::LocalUrl},
    git_ext::{self, OneLevel},
    identities::payload,
    internal::canonical::Cstring,
    std_ext::result::ResultExt as _,
};

use super::{AsPayload, CreateRepo};
use crate::{git, sealed::Sealed};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the directory at `{0}` is not a git repository, did you provide the correct path?")]
    NotARepo(PathBuf),

    #[error("the path provided `{0}` does not exist, did you provide the correct path?")]
    PathDoesNotExist(PathBuf),

    #[error(transparent)]
    Ref(#[from] git_ext::name::Error),

    #[error(transparent)]
    Validation(#[from] git::validation::Error),

    #[error(transparent)]
    Git(#[from] git2::Error),

    #[error(transparent)]
    GitInternal(#[from] git::Error),
}

/// For construction, use [`Repot::new`] followed by [`Repot::validate`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repot<V> {
    payload: payload::Project,
    path: PathBuf,
    valid: V,
}

impl<V> Sealed for Repot<V> {}

impl<V> AsPayload for Repot<V> {
    fn as_payload(&self) -> payload::Project {
        self.payload.clone()
    }
}

impl CreateRepo for Repot<Valid> {
    type Error = Error;

    fn init<F>(self, url: LocalUrl, transport: F) -> Result<git2::Repository, Self::Error>
    where
        F: CanOpenStorage + Clone + 'static,
    {
        self.init(url, transport)
    }
}

impl<V> Repot<V> {
    pub fn name(&self) -> &Cstring {
        &self.payload.name
    }
}

pub type Invalid = PhantomData<!>;

impl Repot<Invalid> {
    pub fn new(payload: payload::Project, path: PathBuf) -> Self {
        Self {
            payload,
            path,
            valid: PhantomData,
        }
    }
}

pub struct Valid {
    repo: git2::Repository,
    default_branch: OneLevel,
}

impl Repot<Valid> {
    pub fn validate(existing: Repot<Invalid>, url: LocalUrl) -> Result<Self, Error> {
        if !existing.path.exists() {
            return Err(Error::PathDoesNotExist(existing.path));
        }

        let repo = git2::Repository::open(existing.path.clone())
            .or_matches(git_ext::is_not_found_err, || {
                Err(Error::NotARepo(existing.path.clone()))
            })?;
        let default_branch = git::determine_default_branch(&existing.payload)?;

        {
            let _default_branch_ref = git::validation::branch(&repo, &default_branch)?;
            let _remote = git::validation::remote(&repo, &url)?;
        }

        Ok(Repot {
            payload: existing.payload,
            path: existing.path,
            valid: Valid {
                repo,
                default_branch,
            },
        })
    }

    pub fn init<F>(self, url: LocalUrl, open_storage: F) -> Result<git2::Repository, Error>
    where
        F: CanOpenStorage + Clone + 'static,
    {
        let Valid {
            repo,
            default_branch,
        } = self.valid;
        tracing::debug!(
            "Setting up existing repository @ '{}'",
            repo.path().display()
        );
        let _remote = git::validation::remote(&repo, &url)?;
        git::setup_remote(&repo, open_storage, url, &default_branch)?;
        Ok(repo)
    }
}
