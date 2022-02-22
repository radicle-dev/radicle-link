// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{io, marker::PhantomData, path::PathBuf};

use serde::{Deserialize, Serialize};

use librad::{
    canonical::Cstring,
    git::local::{transport::CanOpenStorage, url::LocalUrl},
    git_ext::OneLevel,
    identities::payload,
};
use std_ext::Void;

use crate::{
    field::{HasBranch, HasName},
    git,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("a file/directory at `{0}` already exists")]
    AlreadyExists(PathBuf),

    #[error(transparent)]
    Git(#[from] git::Error),

    #[error(transparent)]
    Io(#[from] io::Error),
}

/// For construction, use [`New::new`] followed by [`New::validate`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct New<V, P> {
    payload: P,
    path: PathBuf,
    valid: V,
}

impl<V, P: HasName> New<V, P> {
    pub fn path(&self) -> PathBuf {
        self.path.join(self.payload.name().as_str())
    }
}

pub type Invalid = PhantomData<Void>;
pub type Valid = PhantomData<Void>;

impl<P> New<Invalid, P> {
    #[allow(clippy::self_named_constructors)]
    pub fn new(payload: P, path: PathBuf) -> Self {
        Self {
            payload,
            path,
            valid: PhantomData,
        }
    }

    pub fn validate(self) -> Result<New<Valid, P>, Error>
    where
        P: HasName,
    {
        let repo_path = self.path();

        if repo_path.is_file() {
            return Err(Error::AlreadyExists(repo_path));
        }

        if repo_path.exists() && repo_path.is_dir() && repo_path.read_dir()?.next().is_some() {
            return Err(Error::AlreadyExists(repo_path));
        }

        Ok(Self {
            payload: self.payload,
            path: self.path,
            valid: PhantomData,
        })
    }
}

impl New<Valid, payload::ProjectPayload> {
    pub fn init<F>(self, url: LocalUrl, open_storage: F) -> Result<git2::Repository, Error>
    where
        F: CanOpenStorage + Clone + 'static,
    {
        let path = self.path();
        let default = self.payload.branch_or_default();
        init(
            path,
            default,
            &self.payload.subject.description,
            url,
            open_storage,
        )
    }
}

impl New<Valid, payload::PersonPayload> {
    pub fn init<F>(self, url: LocalUrl, open_storage: F) -> Result<git2::Repository, Error>
    where
        F: CanOpenStorage + Clone + 'static,
    {
        let path = self.path();
        let default = self.payload.branch_or_default();
        init(path, default, &None, url, open_storage)
    }
}

fn init<F>(
    path: PathBuf,
    default: OneLevel,
    description: &Option<Cstring>,
    url: LocalUrl,
    open_storage: F,
) -> Result<git2::Repository, Error>
where
    F: CanOpenStorage + Clone + 'static,
{
    tracing::debug!("Setting up new repository @ '{}'", path.display());
    let repo = git::init(path, description, &default)?;
    git::initial_commit(
        &repo,
        &default,
        &git2::Signature::now("Radicle Automated", "Radicle Automated").map_err(git::Error::Git)?,
    )?;
    let remote = git::setup_remote(&repo, open_storage, url, &default)?;
    git::set_upstream(&repo, &remote, default)?;

    Ok(repo)
}
