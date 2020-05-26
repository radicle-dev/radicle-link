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

use std::ops::{Deref, Range};

use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

use crate::{
    git::{
        ext::{is_not_found_err, Git2ErrorExt, References},
        repo::{self, Repo},
        types::Reference,
        url::GitUrlRef,
    },
    internal::borrow::TryToOwned,
    keys::SecretKey,
    meta::entity::{
        data::{EntityBuilder, EntityData},
        Draft,
        Entity,
    },
    paths::Paths,
    peer::PeerId,
    uri::{RadUrl, RadUrn},
};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Blob {0} not found")]
    NoSuchBlob(String),

    #[error("Branch {0} not found")]
    NoSuchBranch(String),

    #[error(transparent)]
    Git(#[from] git2::Error),
}

pub struct Storage {
    pub(super) backend: git2::Repository,
    pub(crate) key: SecretKey,
}

impl Deref for Storage {
    type Target = git2::Repository;

    fn deref(&self) -> &Self::Target {
        &self.backend
    }
}

impl AsRef<git2::Repository> for Storage {
    fn as_ref(&self) -> &git2::Repository {
        self
    }
}

impl Storage {
    pub fn open(paths: &Paths, key: SecretKey) -> Result<Self, Error> {
        git2::Repository::open_bare(paths.git_dir())
            .map(|backend| Self { backend, key })
            .map_err(|e| e.into())
    }

    /// Obtain a new, owned handle to the backing store.
    pub fn reopen(&self) -> Result<Self, Error> {
        self.try_to_owned()
    }

    pub fn init(paths: &Paths, key: SecretKey) -> Result<Self, Error> {
        let repo = git2::Repository::init_opts(
            paths.git_dir(),
            git2::RepositoryInitOptions::new()
                .bare(true)
                .no_reinit(true)
                .external_template(false),
        )?;

        let mut config = repo.config()?;
        config.set_str("user.name", "radicle")?;
        config.set_str("user.email", &format!("radicle@{}", PeerId::from(&key)))?;

        Ok(Self { backend: repo, key })
    }

    pub fn create_repo<T>(&self, meta: &Entity<T, Draft>) -> Result<Repo, repo::Error>
    where
        T: Serialize + DeserializeOwned + Clone + Default,
        EntityData<T>: EntityBuilder,
    {
        Repo::create(self, meta)
    }

    pub fn open_repo(&self, urn: RadUrn) -> Result<Repo, repo::Error> {
        Repo::open(self, urn)
    }

    /// Attempt to clone the designated repo from the network.
    ///
    /// Note that this method **must** be spawned on a `async` runtime, where
    /// currently the only supported method is [`tokio::task::spawn_blocking`].
    pub fn clone_repo<T>(&self, url: RadUrl) -> Result<Repo, repo::Error>
    where
        T: Serialize + DeserializeOwned + Clone + Default,
        EntityData<T>: EntityBuilder,
    {
        Repo::clone(self, url)
    }

    // Utils

    pub fn has_commit(&self, urn: &RadUrn, oid: git2::Oid) -> Result<bool, Error> {
        let span = tracing::warn_span!("Storage::has_commit", urn = %urn, oid = %oid);
        let _guard = span.enter();

        if oid.is_zero() {
            return Ok(false);
        }

        let commit = self.backend.find_commit(oid);
        match commit {
            Err(e) if is_not_found_err(&e) => {
                tracing::warn!("commit not found");
                Ok(false)
            },
            Ok(commit) => {
                let namespace = &urn.id;
                let branch = urn.path.deref_or_default();
                let branch = branch.strip_prefix("refs/").unwrap_or(branch);

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
            Err(e) => Err(e.into()),
        }
    }

    pub fn has_ref(&self, reference: &Reference) -> Result<bool, Error> {
        self.backend
            .find_reference(&reference.to_string())
            .map(|_| true)
            .map_not_found(|| Ok(false))
    }

    pub fn has_urn(&self, urn: &RadUrn) -> Result<bool, Error> {
        let namespace = &urn.id;
        let branch = urn.path.deref_or_default();
        let branch = branch.strip_prefix("refs/").unwrap_or(branch);
        self.backend
            .find_reference(&format!("refs/namespaces/{}/refs/{}", namespace, branch))
            .map(|_| true)
            .map_not_found(|| Ok(false))
    }

    pub fn track(&self, urn: &RadUrn, peer: &PeerId) -> Result<(), Error> {
        let remote_name = tracking_remote_name(urn, peer);
        let url = GitUrlRef::from_rad_urn(&urn, &PeerId::from(&self.key), peer).to_string();

        tracing::debug!(
            urn = %urn,
            peer = %peer,
            "Storage::track"
        );

        let _ = self.backend.remote(&remote_name, &url)?;
        Ok(())
    }

    pub fn untrack(&self, urn: &RadUrn, peer: &PeerId) -> Result<(), Error> {
        let remote_name = tracking_remote_name(urn, peer);
        // TODO: This removes all remote tracking branches matching the
        // fetchspec (I suppose). Not sure this is what we want.
        self.backend
            .remote_delete(&remote_name)
            .map_err(|e| e.into())
    }

    pub fn tracked(&self, urn: &RadUrn) -> Result<Tracked, Error> {
        Tracked::collect(&self.backend, urn).map_err(|e| e.into())
    }
}

impl TryToOwned for Storage {
    type Owned = Self;
    type Error = Error;

    fn try_to_owned(&self) -> Result<Self::Owned, Self::Error> {
        let backend = self.backend.try_to_owned()?;
        let key = self.key.clone();
        Ok(Self { backend, key })
    }
}

/// Iterator over the 1st degree tracked peers of a repo.
///
/// Created by the [`Storage::tracked`] method.
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct Tracked {
    remotes: git2::string_array::StringArray,
    range: Range<usize>,
    prefix: String,
}

impl Tracked {
    pub(super) fn collect(repo: &git2::Repository, context: &RadUrn) -> Result<Self, git2::Error> {
        let remotes = repo.remotes()?;
        let range = 0..remotes.len();
        let prefix = format!("{}/", context.id);
        Ok(Self {
            remotes,
            range,
            prefix,
        })
    }
}

impl Iterator for Tracked {
    type Item = PeerId;

    fn next(&mut self) -> Option<Self::Item> {
        let next = self.range.next().and_then(|i| self.remotes.get(i));
        match next {
            None => None,
            Some(name) => {
                let peer = name
                    .strip_prefix(&self.prefix)
                    .and_then(|peer| peer.parse().ok());
                peer.or_else(|| self.next())
            },
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.range.size_hint()
    }
}

pub enum WithBlob<'a> {
    Tip {
        reference: &'a Reference,
        file_name: &'a str,
    },
    Init {
        reference: &'a Reference,
        file_name: &'a str,
    },
}

impl<'a> WithBlob<'a> {
    pub fn get(self, git: &'a git2::Repository) -> Result<git2::Blob<'a>, Error> {
        match self {
            Self::Tip {
                reference,
                file_name,
            } => {
                let ref_name = reference.to_string();
                let branch = git
                    .find_reference(&ref_name)
                    .map_not_found(|| Err(Error::NoSuchBranch(ref_name)))?;
                let tree = branch.peel_to_tree()?;
                blob(git, tree, file_name)
            },

            Self::Init {
                reference,
                file_name,
            } => {
                let mut revwalk = git.revwalk()?;
                let mut sort = git2::Sort::TOPOLOGICAL;
                sort.insert(git2::Sort::REVERSE);
                revwalk.set_sorting(sort)?;
                revwalk.simplify_first_parent()?;
                revwalk.push_ref(&reference.to_string())?;

                match revwalk.next() {
                    None => Err(Error::NoSuchBlob(file_name.to_owned())),
                    Some(oid) => {
                        let oid = oid?;
                        let tree = git.find_commit(oid)?.tree()?;
                        blob(git, tree, file_name)
                    },
                }
            },
        }
    }
}

fn blob<'a>(
    repo: &'a git2::Repository,
    tree: git2::Tree<'a>,
    file_name: &'a str,
) -> Result<git2::Blob<'a>, Error> {
    let entry = tree
        .get_name(file_name)
        .ok_or_else(|| Error::NoSuchBlob(file_name.to_owned()))?;
    let bob = entry.to_object(repo)?.peel_to_blob()?;

    Ok(bob)
}

fn tracking_remote_name(urn: &RadUrn, peer: &PeerId) -> String {
    format!("{}/{}", urn.id, peer)
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    use crate::{
        hash::Hash,
        uri::{self, RadUrn},
    };

    #[test]
    fn test_tracking_read_after_write() {
        let tmp = tempdir().unwrap();
        let paths = Paths::from_root(tmp).unwrap();
        let key = SecretKey::new();
        let store = Storage::init(&paths, key).unwrap();

        let urn = RadUrn {
            id: Hash::hash(b"lala"),
            proto: uri::Protocol::Git,
            path: uri::Path::empty(),
        };
        let peer = PeerId::from(SecretKey::new());

        store.track(&urn, &peer).unwrap();
        let tracked = store.tracked(&urn).unwrap().next();
        assert_eq!(tracked, Some(peer))
    }

    #[test]
    fn test_untrack() {
        let tmp = tempdir().unwrap();
        let paths = Paths::from_root(tmp).unwrap();
        let key = SecretKey::new();
        let store = Storage::init(&paths, key).unwrap();

        let urn = RadUrn {
            id: Hash::hash(b"lala"),
            proto: uri::Protocol::Git,
            path: uri::Path::empty(),
        };
        let peer = PeerId::from(SecretKey::new());

        store.track(&urn, &peer).unwrap();
        store.untrack(&urn, &peer).unwrap();

        assert!(store.tracked(&urn).unwrap().next().is_none())
    }
}
