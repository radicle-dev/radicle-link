// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{fmt, marker::PhantomData, path::PathBuf};

use serde::{Deserialize, Serialize};

use librad::{
    git::local::{transport::CanOpenStorage, url::LocalUrl},
    git_ext,
    std_ext::result::ResultExt as _,
};
use std_ext::Void;

use crate::{field::HasBranch, git};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the directory at `{0}` is not a git repository, did you provide the correct path?")]
    NotARepo(PathBuf),

    #[error("the path provided `{0}` does not exist, did you provide the correct path?")]
    PathDoesNotExist(PathBuf),

    #[error(transparent)]
    Validation(#[from] git::validation::Error),

    #[error(transparent)]
    Git(#[from] git2::Error),

    #[error(transparent)]
    GitInternal(#[from] git::Error),
}

/// For construction, use [`Existing::new`] followed by [`Existing::validate`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Existing<V, P> {
    payload: P,
    path: PathBuf,
    valid: V,
}

type Invalid = PhantomData<Void>;

impl<P: HasBranch> Existing<Invalid, P> {
    pub fn new(payload: P, path: PathBuf) -> Self {
        Self {
            payload,
            path,
            valid: PhantomData,
        }
    }

    pub fn validate(self) -> Result<Existing<Valid, P>, Error> {
        if !self.path.exists() {
            return Err(Error::PathDoesNotExist(self.path));
        }

        let repo = git2::Repository::open(self.path.clone())
            .or_matches(git_ext::is_not_found_err, || {
                Err(Error::NotARepo(self.path.clone()))
            })?;

        {
            let _default_branch_ref =
                git::validation::branch(&repo, &self.payload.branch_or_default())?;
        }

        Ok(Existing {
            payload: self.payload,
            path: self.path,
            valid: Valid { repo },
        })
    }
}

/// A validated git Repository.
///
/// Note: the `Debug` implementation prints the path to the repository.
pub struct Valid {
    repo: git2::Repository,
}

impl fmt::Debug for Valid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Valid {{ repo: {} }}", self.repo.path().display())
    }
}

impl<P: HasBranch> Existing<Valid, P> {
    pub fn init<F>(self, url: LocalUrl, open_storage: F) -> Result<git2::Repository, Error>
    where
        F: CanOpenStorage + Clone + 'static,
    {
        let Valid { repo } = self.valid;
        tracing::debug!(
            "Setting up existing repository @ '{}'",
            repo.path().display()
        );
        let _remote = git::validation::remote(&repo, &url)?;
        git::setup_remote(&repo, open_storage, url, &self.payload.branch_or_default())?;

        Ok(repo)
    }
}
