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

use std::{
    ops::{Deref, Range},
    sync::{Arc, Mutex, MutexGuard},
};

use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

use crate::{
    git::{
        ext::is_not_found_err,
        repo::{self, Repo},
        types::Reference,
        url::GitUrlRef,
    },
    keys::SecretKey,
    meta::entity::{
        data::{EntityBuilder, EntityData},
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

#[derive(Clone)]
pub struct Storage {
    backend: Arc<Mutex<git2::Repository>>,
    pub(crate) key: SecretKey,
}

impl Storage {
    pub fn open(paths: &Paths, key: SecretKey) -> Result<Self, Error> {
        git2::Repository::open_bare(paths.git_dir())
            .map(|backend| Self {
                backend: Arc::new(Mutex::new(backend)),
                key,
            })
            .map_err(|e| e.into())
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

        Ok(Self {
            backend: Arc::new(Mutex::new(repo)),
            key,
        })
    }

    pub fn create_repo<T>(self, meta: &Entity<T>) -> Result<Repo, repo::Error>
    where
        T: Serialize + DeserializeOwned + Clone + Default,
        EntityData<T>: EntityBuilder,
    {
        Repo::create(self, meta)
    }

    pub fn open_repo(self, urn: RadUrn) -> Result<Repo, repo::Error> {
        Repo::open(self, urn)
    }

    pub fn clone_repo<T>(self, url: RadUrl) -> Result<Repo, repo::Error>
    where
        T: Serialize + DeserializeOwned + Clone + Default,
        EntityData<T>: EntityBuilder,
    {
        Repo::clone(self, url)
    }

    // Utils

    pub(super) fn lock(&self) -> MutexGuard<git2::Repository> {
        self.backend.lock().unwrap()
    }

    pub(crate) fn has_commit(&self, urn: &RadUrn, oid: git2::Oid) -> Result<bool, Error> {
        let span = tracing::warn_span!("Storage::has_commit", urn = %urn, oid = %oid);
        let _guard = span.enter();

        if oid.is_zero() {
            return Ok(false);
        }

        let git = self.lock();
        let commit = git.find_commit(oid);
        match commit {
            Err(e) if is_not_found_err(&e) => {
                tracing::warn!("commit not found");
                Ok(false)
            },
            Ok(commit) => {
                let namespace = &urn.id;
                let branch = if urn.path.is_empty() {
                    "rad/id"
                } else {
                    urn.path.deref()
                };

                // FIXME: this is both too expensive and to stringly. We need
                // to be able to tell from the gossip message if we should look
                // in the owned refs or a remote
                let own_refs =
                    git.references_glob(&format!("refs/namespaces/{}/{}", namespace, branch))?;
                let remote_refs = git.references_glob(&format!(
                    "refs/namespaces/{}/refs/remotes/**/{}",
                    namespace,
                    branch.strip_prefix("refs/").unwrap_or(branch)
                ))?;

                for tip in own_refs.chain(remote_refs) {
                    let tip = tip?;
                    if let Some(tip_oid) = tip.target() {
                        if tip_oid == commit.id()
                            || git.graph_descendant_of(tip_oid, commit.id())?
                        {
                            return Ok(true);
                        }
                    }
                }

                Ok(false)
            },
            Err(e) => Err(e.into()),
        }
    }

    pub(crate) fn has_ref(&self, reference: &Reference) -> Result<bool, Error> {
        let git = self.lock();
        git.find_reference(&reference.to_string())
            .map(|_| true)
            .or_else(|e| {
                if is_not_found_err(&e) {
                    Ok(false)
                } else {
                    Err(e.into())
                }
            })
    }

    pub(crate) fn track(&self, urn: &RadUrn, peer: &PeerId) -> Result<(), Error> {
        let remote_name = tracking_remote_name(urn, peer);
        let url = GitUrlRef::from_rad_urn(&urn, &PeerId::from(&self.key), peer).to_string();

        tracing::debug!(
            "Storage::track({}, {}): {} url={}",
            urn,
            peer,
            remote_name,
            url
        );

        let _ = self.lock().remote(&remote_name, &url)?;
        Ok(())
    }

    pub(crate) fn untrack(&self, urn: &RadUrn, peer: &PeerId) -> Result<(), Error> {
        let remote_name = tracking_remote_name(urn, peer);
        // TODO: This removes all remote tracking branches matching the
        // fetchspec (I suppose). Not sure this is what we want.
        self.lock()
            .remote_delete(&remote_name)
            .map_err(|e| e.into())
    }

    pub(crate) fn tracked(&self, urn: &RadUrn) -> Result<Tracked, Error> {
        let remotes = self.lock().remotes()?;
        Ok(Tracked::new(remotes, urn))
    }
}

/// Iterator over the 1st degree tracked peers of a repo.
///
/// Created by the [`Storage::tracked`] method.
pub struct Tracked {
    remotes: git2::string_array::StringArray,
    range: Range<usize>,
    prefix: String,
}

impl Tracked {
    pub(super) fn new(remotes: git2::string_array::StringArray, filter: &RadUrn) -> Self {
        let range = 0..remotes.len();
        let prefix = format!("{}/", filter.id);
        Self {
            remotes,
            range,
            prefix,
        }
    }
}

impl Iterator for Tracked {
    type Item = PeerId;

    fn next(&mut self) -> Option<Self::Item> {
        self.range
            .next()
            .and_then(|i| self.remotes.get(i))
            .and_then(|name| name.strip_prefix(&self.prefix))
            .and_then(|peer| peer.parse().ok())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.range.size_hint()
    }
}

#[derive(Clone, Copy)]
pub enum BranchEnd {
    Tip,
    First,
}

pub struct WithBlob<'a> {
    pub reference: &'a Reference,
    pub file_name: &'a str,
    pub branch_end: BranchEnd,
}

impl<'a> WithBlob<'a> {
    pub fn get(self, git: &'a git2::Repository) -> Result<git2::Blob<'a>, Error> {
        let tree = match self.branch_end {
            BranchEnd::Tip => {
                let ref_name = self.reference.to_string();
                let branch = git.find_reference(&ref_name).or_else(|e| {
                    if is_not_found_err(&e) {
                        Err(Error::NoSuchBranch(ref_name))
                    } else {
                        Err(e.into())
                    }
                })?;
                let tree = branch.peel_to_tree()?;
                Ok(tree)
            },

            BranchEnd::First => {
                let mut revwalk = git.revwalk()?;
                let mut sort = git2::Sort::TOPOLOGICAL;
                sort.insert(git2::Sort::REVERSE);
                revwalk.set_sorting(sort)?;
                revwalk.simplify_first_parent()?;
                revwalk.push_ref(&self.reference.to_string())?;

                match revwalk.next() {
                    None => Err(Error::NoSuchBlob(self.file_name.to_owned())),
                    Some(oid) => {
                        let oid = oid?;
                        let tree = git.find_commit(oid)?.tree()?;
                        Ok(tree)
                    },
                }
            },
        }?;

        let entry = tree
            .get_name(self.file_name)
            .ok_or_else(|| Error::NoSuchBlob(self.file_name.to_owned()))?;
        let blob = entry.to_object(&*git)?.peel_to_blob()?;

        Ok(blob)
    }
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

        assert!(store
            .tracked(&urn)
            .unwrap()
            .collect::<Vec<PeerId>>()
            .is_empty())
    }
}
