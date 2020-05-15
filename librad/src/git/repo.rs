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
    collections::{BTreeMap, HashMap, HashSet},
    fs,
};

use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

use crate::{
    canonical::{Cjson, CjsonError},
    git::{
        refs::{self, Oid, Refs},
        url::GitUrlRef,
    },
    hash::{self, Hash},
    keys::device,
    meta::entity::{
        self,
        data::{EntityBuilder, EntityData},
        Draft,
        Entity,
        Signatory,
    },
    paths::Paths,
    peer::PeerId,
    uri::{self, RadUrl, RadUrn},
};

const RAD_REFS: &str = "rad/refs";

/// A git repository with `radicle-link` specific operations
pub struct Repo {
    urn: RadUrn,
    key: device::Key,
    repo: git2::Repository,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("Repo already exists")]
    AlreadyExists,

    #[error("Unknown repo")]
    NoSuchRepo,

    #[error("Branch {0} not found")]
    NoSuchBranch(String),

    #[error("Blob {0} not found")]
    NoSuchBlob(String),

    #[error(
        "Identity root hash doesn't match resolved URL. Expected {expected}, actual: {actual}"
    )]
    RootHashMismatch { expected: Hash, actual: Hash },

    #[error(transparent)]
    HashAlgorithm(#[from] hash::AlgorithmMismatch),

    #[error(transparent)]
    Entity(#[from] entity::Error),

    #[error(transparent)]
    Refsig(#[from] refs::signed::Error),

    #[error(transparent)]
    Json(#[from] serde_json::error::Error),

    #[error(transparent)]
    Cjson(#[from] CjsonError),

    #[error(transparent)]
    Git(#[from] git2::Error),
}

impl Repo {
    /// Create a [`Repo`] from the given metadata [`Entity`]
    pub fn create<T, ST>(
        paths: &Paths,
        key: device::Key,
        meta: &Entity<T, ST>,
    ) -> Result<Self, Error>
    where
        T: Serialize + DeserializeOwned + Clone + Default,
        ST: Clone,
    {
        /*
        if !meta.status().signed() {
            return Err(Error::Verification {
                expected: VerificationStatus::Signed,
                actual: meta.status().to_owned(),
            });
        }
        */

        let hash = meta.root_hash().to_owned();
        let repo = init_repo(paths, &key, &hash)?;

        {
            let canonical_data: Vec<u8> = meta.to_data().canonical_data()?;
            let blob = repo.blob(&canonical_data)?;
            let tree = {
                let mut builder = repo.treebuilder(None)?;
                builder.insert("id", blob, 0o100_644)?;
                let oid = builder.write()?;
                repo.find_tree(oid)
            }?;

            let author = repo.signature()?;

            repo.commit(
                Some("refs/heads/rad/id"),
                &author,
                &author,
                "Initial identity",
                &tree,
                &[],
            )?;
        }

        let this = Self {
            urn: RadUrn::new(hash, uri::Protocol::Git, uri::Path::empty()),
            key,
            repo,
        };
        this.track_entity(&meta)?;
        this.update_refs()?;

        Ok(this)
    }

    /// Open a [`Repo`] from local storage, identified by the given [`RadUrn`]
    ///
    /// Note that it is not currently validated that the repo is conforming to
    /// `radicle-link` conventions.
    pub fn open(paths: &Paths, key: device::Key, urn: RadUrn) -> Result<Self, Error> {
        let mut repo_path = paths.projects_dir().join(urn.id.to_string());
        repo_path.set_extension("git");

        let repo = git2::Repository::open_bare(repo_path).map_err(|e| {
            if is_not_found(&e) {
                Error::NoSuchRepo
            } else {
                Error::Git(e)
            }
        })?;

        Ok(Self {
            urn: RadUrn {
                path: uri::Path::empty(),
                ..urn
            },
            key,
            repo,
        })
    }

    /// Clone a [`Repo`] from the given [`RadUrl`]
    ///
    /// The repo must not already exist.
    ///
    /// TODO: entity verification is not currently functional, pending #95
    pub fn clone<T>(paths: &Paths, key: device::Key, url: RadUrl) -> Result<Self, Error>
    where
        T: Serialize + DeserializeOwned + Clone + Default,
        EntityData<T>: EntityBuilder,
    {
        let repo = init_repo(paths, &key, &url.urn.id)?;

        // Fetch the identity branch first
        let id_branch = format!("refs/remotes/{}/rad/id", url.authority);
        let peer_id = PeerId::from(&key);
        let git_url = GitUrlRef::from_rad_url_ref(url.as_ref(), &peer_id);
        let entity: Entity<T, Draft> = {
            let mut remote = repo.remote_anonymous(&git_url.to_string())?;
            remote.fetch(
                &[&format!(
                    "+refs/heads/rad/id:refs/remotes/{}/rad/id",
                    url.authority
                )],
                None,
                None,
            )?;

            let id_blob = read_blob_at_init(&repo, &id_branch, "id")?;
            Entity::<T, Draft>::from_json_slice(id_blob.content())
        }?;

        // TODO:
        //
        // * collect `entity.certifiers()`
        //
        //   These are URNs -- either pointing to branches `url` has, or
        //   top-level repos which `url.authority()` may or may not have. This
        //   is complicated. Even more so since we should actually do this for
        //   all certifiers of the entire history of `entity`.
        //
        // * call `entity.compute_status()` and assert it yields
        //   `VerificationStatus::Signed`
        //
        //   Pending an impl of `Resolver`

        if entity.root_hash() != &url.urn.id {
            return Err(Error::RootHashMismatch {
                expected: url.urn.id.to_owned(),
                actual: entity.root_hash().to_owned(),
            });
        }

        // Set our own rad/id to the one we got from `url.authority()`
        // FIXME: this is wrong -- we haven't validated anything. Instead, we
        // need to fetch, determine the latest valid revision, and create a new
        // commit of our own using any of the certifiers' trees.
        {
            let id_head_oid = repo.refname_to_id(&id_branch)?;
            let id_head = repo.find_commit(id_head_oid)?;
            repo.branch("rad/id", &id_head, false)?;
        }

        let this = Self {
            urn: RadUrn {
                path: uri::Path::empty(),
                ..url.urn
            },
            key,
            repo,
        };
        this.track_entity(&entity)?;
        this.fetch(&url.authority)?;

        Ok(this)
    }

    /// Fetch updates from peer [`PeerId`], if any
    ///
    /// Note that verification of any entity updates is not performed here.
    pub fn fetch(&self, from: &PeerId) -> Result<(), Error> {
        let mut remote = self.repo.remote_anonymous(
            &GitUrlRef::from_rad_url_ref(self.urn().as_rad_url_ref(from), &PeerId::from(&self.key))
                .to_string(),
        )?;
        remote.connect(git2::Direction::Fetch)?;

        let refs = self.rad_refs()?;
        let all_remotes = refs.remotes.flatten().collect::<HashSet<&PeerId>>();
        // Fetch rad/refs of all known remotes, rejecting non-fast-forwards
        {
            let refspecs = all_remotes
                .iter()
                .map(|peer| {
                    if peer == &from {
                        format!("refs/heads/rad/refs:refs/remotes/{}/rad/refs", peer)
                    } else {
                        format!(
                            "refs/remotes/{}/rad/refs:refs/remotes/{}/rad/refs",
                            peer, peer
                        )
                    }
                })
                .collect::<Vec<String>>();

            remote.fetch(&refspecs, None, None)?;
        }

        // Read the signed refs of all known remotes, and compare their `heads` against
        // the advertised refs. If signed and advertised branch head matches,
        // non-fast-forwards are permitted. Otherwise, the branch is skipped.
        {
            let remote_heads: HashMap<&str, &git2::RemoteHead> = remote
                .list()?
                .iter()
                .map(|rhead| (rhead.name(), rhead))
                .collect();
            let refspecs = all_remotes
                .iter()
                .filter_map(|peer| {
                    self.rad_refs_for(peer).ok().map(|refs| {
                        refs.heads
                            .iter()
                            .filter_map(|(name, target)| {
                                let good = remote_heads
                                    .get(name.as_str())
                                    .map(|rtarget| rtarget.oid() == **target)
                                    .unwrap_or(false);

                                if good {
                                    if peer == &from {
                                        Some(format!(
                                            "+refs/heads/{}:refs/remotes/{}/{}",
                                            name, peer, name
                                        ))
                                    } else {
                                        Some(format!(
                                            "+refs/remotes/{}/{}:refs/remotes/{}/{}",
                                            peer, name, peer, name
                                        ))
                                    }
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<String>>()
                    })
                })
                .flatten()
                .collect::<Vec<String>>();

            remote.fetch(&refspecs, None, None)?;
        }

        // At this point, the transitive tracking graph may have changed. Let's update
        // the refs, but don't recurse here for now (we could, if we reload
        // `self.refs()` and compare to the value we had before fetching).
        self.update_refs()
    }

    pub fn urn(&self) -> &RadUrn {
        &self.urn
    }

    /// Track [`PeedId`]
    pub fn track(&self, peer: PeerId) -> Result<(), Error> {
        self.repo
            .remote_with_fetch(
                &peer.to_string(),
                &GitUrlRef::from_rad_urn(&self.urn, &PeerId::from(&self.key), &peer).to_string(),
                &format!("refs/heads/*:refs/remotes/{}/*", peer),
            )
            .map(|_| ())
            .map_err(|e| e.into())
    }

    /// Internal: track all external signers of `Entity`
    fn track_entity<T, ST>(&self, entity: &Entity<T, ST>) -> Result<(), Error>
    where
        T: Serialize + DeserializeOwned + Clone + Default,
        ST: Clone,
    {
        let signatures = entity
            .signatures()
            .iter()
            .filter_map(|(pk, sig)| match &sig.by {
                Signatory::User(urn) => Some((pk, urn)),
                Signatory::OwnedKey => None,
            });

        let local_peer = PeerId::from(&self.key);
        for (pk, urn) in signatures {
            let peer = PeerId::from(pk.clone());
            let remote_name = peer.to_string();
            let url = GitUrlRef::from_rad_url_ref(urn.as_rad_url_ref(&peer), &local_peer);

            self.repo
                .remote(&remote_name, &url.to_string())
                .map(|_| ())
                .or_else(|e| if is_exists(&e) { Ok(()) } else { Err(e) })?;
        }

        Ok(())
    }

    /// Untrack [`PeerId`]
    pub fn untrack(&self, peer: &PeerId) -> Result<(), Error> {
        self.repo
            .remote_delete(&peer.to_string())
            .map_err(|e| e.into())
    }

    /// Determine if [`PeerId`] is directly tracked
    pub fn tracks(&self, peer: &PeerId) -> Result<bool, Error> {
        self.repo
            .find_remote(&peer.to_string())
            .map(|_| true)
            .or_else(|e| {
                if is_not_found(&e) {
                    Ok(false)
                } else {
                    Err(e.into())
                }
            })
    }

    /// Retrieve all _directly_ tracked peers
    ///
    /// To retrieve the transitively tracked peers, use [`rad_refs`] and inspect
    /// the `remotes`.
    pub fn tracked(&self) -> Result<Vec<PeerId>, Error> {
        let remotes = self.repo.remotes()?;
        Ok(remotes
            .iter()
            .filter_map(|name| name.and_then(|s| s.parse().ok()))
            .collect())
    }

    /// Determine if the given object is in the object database
    pub fn has_object(&self, oid: git2::Oid) -> Result<bool, Error> {
        self.repo.find_object(oid, None).map(|_| true).or_else(|e| {
            if is_not_found(&e) {
                Ok(false)
            } else {
                Err(e.into())
            }
        })
    }

    /// Read the current [`Refs`] from the repo state
    pub fn rad_refs(&self) -> Result<Refs, Error> {
        // Collect refs/heads (our branches) at their current state
        let mut heads = BTreeMap::new();
        {
            let branches = self.repo.branches(Some(git2::BranchType::Local))?;
            for res in branches {
                let (branch, _) = res?;
                if let Some(name) = branch.name()? {
                    let name = name.to_owned();
                    if let Some(target) = branch.into_reference().target() {
                        heads.insert(name, Oid(target.to_owned()));
                    }
                }
            }
        }

        // Get 1st degree tracked peers from the remotes configured in .git/config
        let git_remotes = self.repo.remotes()?;
        let mut remotes: HashMap<PeerId, HashMap<PeerId, HashSet<PeerId>>> = git_remotes
            .iter()
            .filter_map(|name| {
                name.and_then(|s| {
                    s.parse::<PeerId>()
                        .map(|peer_id| (peer_id, HashMap::new()))
                        .ok()
                })
            })
            .collect();

        // For each of the 1st degree tracked peers, lookup their rad/refs (if any),
        // verify the signature, and add their [`Remotes`] to ours (minus the 3rd
        // degree)
        for (peer, tracked) in remotes.iter_mut() {
            match self.rad_refs_for(peer) {
                Ok(refs) => *tracked = refs.remotes.cutoff(),
                Err(e) => match e {
                    Error::NoSuchBranch(_) | Error::NoSuchBlob(_) => {},
                    _ => return Err(e),
                },
            }
        }

        Ok(Refs {
            heads,
            remotes: remotes.into(),
        })
    }

    fn update_refs(&self) -> Result<(), Error> {
        let refsig_canonical = self
            .rad_refs()?
            .sign(&self.key)
            .and_then(|signed| Cjson(signed).canonical_form())?;

        let parent: Option<git2::Commit> = {
            self.repo
                .find_branch(RAD_REFS, git2::BranchType::Local)
                .and_then(|branch| branch.into_reference().peel_to_commit().map(Some))
                .or_else(|e| if is_not_found(&e) { Ok(None) } else { Err(e) })
        }?;
        let blob = self.repo.blob(&refsig_canonical)?;
        let tree = {
            let mut builder = self.repo.treebuilder(None)?;
            builder.insert("refs", blob, 0o100_644)?;
            let oid = builder.write()?;
            self.repo.find_tree(oid)
        }?;

        // Don't create a new commit if it would be the same tree as the parent
        if let Some(ref parent) = parent {
            if parent.tree()?.id() == tree.id() {
                return Ok(());
            }
        }

        let author = self.repo.signature()?;

        self.repo.commit(
            Some(RAD_REFS),
            &author,
            &author,
            "",
            &tree,
            &parent.iter().collect::<Vec<&git2::Commit>>(),
        )?;

        Ok(())
    }

    fn rad_refs_for(&self, peer: &PeerId) -> Result<Refs, Error> {
        let ref_name = format!("refs/remotes/{}/rad/refs", peer);
        let blob = read_blob_at_tip(&self.repo, &ref_name, "refs")?;
        let signed = refs::Signed::from_json(blob.content(), &peer)?;

        Ok(Refs::from(signed))
    }
}

impl AsRef<git2::Repository> for Repo {
    fn as_ref(&self) -> &git2::Repository {
        &self.repo
    }
}

fn is_not_found(e: &git2::Error) -> bool {
    e.code() == git2::ErrorCode::NotFound
}

fn is_exists(e: &git2::Error) -> bool {
    e.code() == git2::ErrorCode::Exists
}

fn init_repo(paths: &Paths, key: &device::Key, id: &Hash) -> Result<git2::Repository, Error> {
    let mut repo_path = paths.projects_dir().join(id.to_string());
    repo_path.set_extension("git");

    let repo = git2::Repository::init_opts(
        &repo_path,
        git2::RepositoryInitOptions::new()
            .bare(true)
            .no_reinit(true)
            .external_template(false),
    )
    .map_err(|e| match e.code() {
        git2::ErrorCode::Exists => Error::AlreadyExists,
        _ => Error::Git(e),
    })?;

    fn set_user_info(repo: &git2::Repository, key: &device::Key) -> Result<(), git2::Error> {
        let mut config = repo.config()?;
        config.set_str("user.name", "radicle")?;
        config.set_str("user.email", &format!("radicle@{}", &key))
    }

    match set_user_info(&repo, key) {
        Ok(()) => Ok(repo),
        Err(e) => {
            drop(repo);
            fs::remove_dir_all(&repo_path).unwrap_or_else(|_| {
                panic!(
                    "Failed to initialize repo at {}. You need to remove the directory manually",
                    &repo_path.display()
                )
            });
            Err(e.into())
        },
    }
}

fn read_blob_at_tip<'a>(
    repo: &'a git2::Repository,
    ref_name: &str,
    file_name: &str,
) -> Result<git2::Blob<'a>, Error> {
    let branch = repo.find_reference(ref_name).or_else(|e| {
        if is_not_found(&e) {
            Err(Error::NoSuchBranch(ref_name.to_owned()))
        } else {
            Err(e.into())
        }
    })?;
    let tree = branch.peel_to_tree()?;
    read_blob_from_tree(repo, tree, file_name)
}

fn read_blob_at_init<'a>(
    repo: &'a git2::Repository,
    ref_name: &str,
    file_name: &str,
) -> Result<git2::Blob<'a>, Error> {
    let mut revwalk = repo.revwalk()?;
    let mut sort = git2::Sort::TOPOLOGICAL;
    sort.insert(git2::Sort::REVERSE);
    revwalk.set_sorting(sort)?;
    revwalk.simplify_first_parent()?;
    revwalk.push_ref(ref_name)?;

    match revwalk.next() {
        None => Err(Error::NoSuchBlob(file_name.to_owned())),
        Some(oid) => {
            let oid = oid?;
            let tree = repo.find_commit(oid)?.tree()?;
            read_blob_from_tree(repo, tree, file_name)
        },
    }
}

fn read_blob_from_tree<'a>(
    repo: &'a git2::Repository,
    tree: git2::Tree,
    file_name: &str,
) -> Result<git2::Blob<'a>, Error> {
    let entry = tree
        .get_name(file_name)
        .ok_or_else(|| Error::NoSuchBlob(file_name.to_owned()))?;
    let blob = entry.to_object(&repo)?.peel_to_blob()?;

    Ok(blob)
}
