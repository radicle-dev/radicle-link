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
    ops::Deref,
    sync::{Arc, Mutex, MutexGuard},
};

use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

use crate::{
    git::{
        ext::is_not_found_err,
        repo::{self, Repo},
        types::{Namespace, Reference, RefsCategory, Refspec},
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

    // FIXME: tests should use a working copy + push
    pub fn create_empty_commit(&self, urn: RadUrn) -> Result<git2::Oid, Error> {
        let git = self.backend.lock().unwrap();

        let tree = {
            let mut index = git.index()?;
            let tree_id = index.write_tree()?;
            git.find_tree(tree_id)
        }?;
        let author = git.signature()?;

        let oid = git.commit(
            Some(
                &Reference {
                    namespace: urn.id,
                    remote: None,
                    category: RefsCategory::Heads,
                    name: "master".to_owned(),
                }
                .to_string(),
            ),
            &author,
            &author,
            "Initial commit",
            &tree,
            &[],
        )?;

        Ok(oid)
    }

    // FIXME: provide namespace-aware delegators instead
    pub(crate) fn backend(&self) -> MutexGuard<git2::Repository> {
        self.backend.lock().unwrap()
    }

    pub(crate) fn has_commit(&self, urn: &RadUrn, oid: git2::Oid) -> Result<bool, Error> {
        if oid.is_zero() {
            return Ok(false);
        }

        let git = self.backend.lock().unwrap();
        let commit = git.find_commit(oid);
        match commit {
            Err(e) if is_not_found_err(&e) => Ok(false),
            Ok(commit) => {
                let namespace = &urn.id;
                let branch = if urn.path.is_empty() {
                    "rad/id"
                } else {
                    urn.path.deref()
                };

                match git.find_reference(&format!("refs/namespaces/{}/{}", namespace, branch)) {
                    Err(e) if is_not_found_err(&e) => Ok(false),
                    Ok(tip) => {
                        match tip.target() {
                            None => Ok(false), // FIXME: ??
                            Some(tip) if tip == commit.id() => Ok(true),
                            Some(tip) => git
                                .graph_descendant_of(tip, commit.id())
                                .map_err(|e| e.into()),
                        }
                    },
                    Err(e) => Err(e.into()),
                }
            },
            Err(e) => Err(e.into()),
        }
    }

    pub(crate) fn has_ref(&self, reference: &Reference) -> Result<bool, Error> {
        let git = self.backend.lock().unwrap();
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

    pub(crate) fn has_remote(&self, name: &str) -> Result<bool, Error> {
        let git = self.backend.lock().unwrap();
        git.find_remote(name).map(|_| true).or_else(|e| {
            if is_not_found_err(&e) {
                Ok(false)
            } else {
                Err(e.into())
            }
        })
    }

    pub(crate) fn track(&self, urn: &RadUrn, peer: PeerId) -> Result<(), Error> {
        let remote_name = tracking_remote_name(urn, &peer);
        let namespace = urn.id.clone();

        let git = self.backend.lock().unwrap();
        if !self.has_remote(&remote_name)? {
            let _ = git.remote_with_fetch(
                &remote_name,
                &GitUrlRef::from_rad_urn(&urn, &PeerId::from(&self.key), &peer).to_string(),
                &track_spec(namespace.clone(), peer.clone(), RefsCategory::Heads).to_string(),
            )?;
        }

        git.remote_add_fetch(
            &remote_name,
            &track_spec(namespace, peer, RefsCategory::Rad).to_string(),
        )?;

        Ok(())
    }

    pub(crate) fn untrack(&self, urn: &RadUrn, peer: PeerId) -> Result<(), Error> {
        let remote_name = tracking_remote_name(urn, &peer);
        // TODO: This removes all remote tracking branches matching the
        // fetchspec (I suppose). Not sure this is what we want.
        let git = self.backend.lock().unwrap();
        git.remote_delete(&remote_name).map_err(|e| e.into())
    }
}

#[derive(Clone, Copy)]
pub enum Side {
    Tip,
    First,
}

pub struct WithBlob<'a> {
    pub reference: &'a Reference,
    pub file_name: &'a str,
    pub side: Side,
}

impl<'a> WithBlob<'a> {
    pub fn get(self, git: &'a git2::Repository) -> Result<git2::Blob<'a>, Error> {
        let tree = match self.side {
            Side::Tip => {
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
            Side::First => {
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
    format!("{}/{}", urn, peer)
}

fn track_spec(namespace: Namespace, peer: PeerId, category: RefsCategory) -> Refspec {
    let remote = Reference {
        namespace,
        remote: None,
        category,
        name: "*".to_owned(),
    };
    let local = Reference {
        remote: Some(peer),
        ..remote.clone()
    };

    Refspec {
        local,
        remote,
        force: false,
    }
}
