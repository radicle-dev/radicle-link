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

use crate::{git, AsPayload, CreateRepo};

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

/// For construction, use [`Existing::new`] followed by [`Existing::validate`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Existing<V> {
    description: Option<Cstring>,
    default_branch: OneLevel,
    path: PathBuf,
    name: Cstring,
    valid: V,
}

impl<V> AsPayload for Existing<V> {
    fn as_payload(&self) -> payload::Project {
        payload::Project {
            default_branch: Some(Cstring::from(self.default_branch.as_str())),
            description: self.description.clone(),
            name: self.name.clone(),
        }
    }
}

impl CreateRepo for Existing<Valid> {
    type Error = Error;

    fn init<F>(self, url: LocalUrl, transport: F) -> Result<git2::Repository, Self::Error>
    where
        F: CanOpenStorage + 'static,
    {
        self.init(url, transport)
    }
}

impl<V> Existing<V> {
    pub fn name(&self) -> &Cstring {
        &self.name
    }
}

type Invalid = PhantomData<!>;

impl Existing<Invalid> {
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

impl Existing<Valid> {
    pub fn validate(existing: Existing<Invalid>) -> Result<Self, Error> {
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

        Ok(Existing {
            description: existing.description,
            default_branch: existing.default_branch,
            path: existing.path,
            name: existing.name,
            valid: Valid { repo },
        })
    }

    pub fn init<F>(self, url: LocalUrl, open_storage: F) -> Result<git2::Repository, Error>
    where
        F: CanOpenStorage + 'static,
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
