// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{marker::PhantomData, path::PathBuf};

use serde::{Deserialize, Serialize};

use librad::{
    canonical::Cstring,
    git::local::{transport::CanOpenStorage, url::LocalUrl},
    git_ext::{self, OneLevel},
    identities::payload,
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
    Validation(#[from] git::validation::Error),

    #[error(transparent)]
    Git(#[from] git2::Error),

    #[error(transparent)]
    GitInternal(#[from] git::Error),
}

#[derive(Debug, thiserror::Error)]
#[error("could not determine the name of the project from the given path `{0}`")]
pub struct EmptyNameError(PathBuf);

/// For construction, use [`Repot::new`] followed by [`Repot::validate`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repot<V> {
    description: Option<Cstring>,
    default_branch: OneLevel,
    path: PathBuf,
    name: Cstring,
    valid: V,
}

impl<V> Sealed for Repot<V> {}

impl<V> AsPayload for Repot<V> {
    fn as_payload(&self) -> payload::Project {
        payload::Project {
            default_branch: Some(Cstring::from(self.default_branch.as_str())),
            description: self.description.clone(),
            name: self.name.clone(),
        }
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
        &self.name
    }
}

type Invalid = PhantomData<!>;

impl Repot<Invalid> {
    pub fn new(
        description: Option<Cstring>,
        default_branch: OneLevel,
        path: PathBuf,
    ) -> Result<Self, EmptyNameError> {
        let name = path
            .components()
            .next_back()
            .and_then(|component| component.as_os_str().to_str())
            .map(ToString::to_string)
            .map(Cstring::from)
            .ok_or_else(|| EmptyNameError(path.clone()))?;
        Ok(Self {
            description,
            default_branch,
            path,
            name,
            valid: PhantomData,
        })
    }
}

pub struct Valid {
    repo: git2::Repository,
}

impl Repot<Valid> {
    pub fn validate(existing: Repot<Invalid>) -> Result<Self, Error> {
        if !existing.path.exists() {
            return Err(Error::PathDoesNotExist(existing.path));
        }

        let repo = git2::Repository::open(existing.path.clone())
            .or_matches(git_ext::is_not_found_err, || {
                Err(Error::NotARepo(existing.path.clone()))
            })?;

        {
            let _default_branch_ref = git::validation::branch(&repo, &existing.default_branch)?;
        }

        Ok(Repot {
            description: existing.description,
            default_branch: existing.default_branch,
            path: existing.path,
            name: existing.name,
            valid: Valid { repo },
        })
    }

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
        git::setup_remote(&repo, open_storage, url, &self.default_branch)?;
        Ok(repo)
    }
}
