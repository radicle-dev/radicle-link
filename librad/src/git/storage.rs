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
    ops::{Deref, Range},
    str::FromStr,
};

use radicle_surf::vcs::git as surf;
use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

use crate::{
    git::{
        ext::{is_exists_err, is_not_found_err, Oid, References},
        refs::{self, Refs},
        repo::Repo,
        types::{Reference, Refspec},
        url::GitUrlRef,
    },
    hash::Hash,
    internal::{
        borrow::TryToOwned,
        canonical::{Cjson, CjsonError},
        result::ResultExt,
    },
    keys::SecretKey,
    meta::entity::{self, data::EntityInfoExt, Draft, Entity, GenericDraftEntity, Signatory},
    paths::Paths,
    peer::PeerId,
    uri::{self, Path, Protocol, RadUrl, RadUrn},
};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Blob {0} not found")]
    NoSuchBlob(String),

    #[error("Branch {0} not found")]
    NoSuchBranch(String),

    #[error("Unknown repo {0}")]
    NoSuchUrn(RadUrn),

    #[error(
        "Identity root hash doesn't match resolved URL. Expected {expected}, actual: {actual}"
    )]
    RootHashMismatch { expected: Hash, actual: Hash },

    #[error("Metadata is not signed")]
    UnsignedMetadata,

    #[error(transparent)]
    Urn(#[from] uri::rad_urn::ParseError),

    #[error(transparent)]
    Entity(#[from] entity::Error),

    #[error(transparent)]
    Refsig(#[from] refs::signed::Error),

    #[error(transparent)]
    Cjson(#[from] CjsonError),

    #[error(transparent)]
    Surf(#[from] surf::error::Error),

    #[error(transparent)]
    Git(#[from] git2::Error),
}

pub struct Storage {
    pub(super) backend: git2::Repository,
    pub(crate) key: SecretKey,
}

// FIXME(kim): we really don't want to export this
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

pub struct LinearHistoryCommits<'a>(Option<git2::Commit<'a>>);

impl<'a> Iterator for LinearHistoryCommits<'a> {
    type Item = git2::Commit<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        let result = self.0.clone();
        self.0 = self.0.as_ref().and_then(|commit| commit.parents().next());
        result
    }
}

impl Storage {
    /// Open the `Storage` found at the given [`Paths`]'s `git_dir`.
    /// If the path does not exist we initialise the `Storage` with
    /// [`Storage::init`].
    pub fn open(paths: &Paths, key: SecretKey) -> Result<Self, Error> {
        git2::Repository::open_bare(paths.git_dir())
            .map(|backend| Self {
                backend,
                key: key.clone(),
            })
            .or_matches(is_not_found_err, || Ok(Self::init(paths, key)?))
    }

    /// Obtain a new, owned handle to the backing store.
    pub fn reopen(&self) -> Result<Self, Error> {
        self.try_to_owned()
    }

    /// Initialise the `Storage` at the given [`Paths`]'s `git_dir`.
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

    pub fn create_repo<T>(&self, meta: &Entity<T, Draft>) -> Result<Repo, Error>
    where
        T: Serialize + DeserializeOwned + Clone + EntityInfoExt,
    {
        let span = tracing::info_span!("Storage::create_repo");
        let _guard = span.enter();

        // FIXME: properly verify meta

        if meta.signatures().is_empty() {
            return Err(Error::UnsignedMetadata);
        }

        // FIXME: certifier identities must exist, or be supplied

        let urn = RadUrn::new(
            meta.root_hash().to_owned(),
            uri::Protocol::Git,
            uri::Path::empty(),
        );

        self.commit_initial_meta(&meta)?;
        self.track_signers(&meta)?;
        self.update_refs(&urn)?;

        Ok(Repo {
            urn,
            storage: self.into(),
        })
    }

    pub fn open_repo(&self, urn: RadUrn) -> Result<Repo, Error> {
        {
            let id_ref = Reference::rad_id(urn.id.clone());
            if !self.has_ref(&id_ref)? {
                return Err(Error::NoSuchUrn(urn));
            }
        }

        Ok(Repo {
            urn: RadUrn {
                path: uri::Path::empty(),
                ..urn
            },
            storage: self.into(),
        })
    }

    /// Attempt to clone the designated repo from the network.
    ///
    /// Note that this method **must** be spawned on a `async` runtime, where
    /// currently the only supported method is [`tokio::task::spawn_blocking`].
    pub fn clone_repo<T>(&self, url: RadUrl) -> Result<Repo, Error>
    where
        T: Serialize + DeserializeOwned + Clone + EntityInfoExt,
    {
        let span = tracing::info_span!("Storage::clone_repo", url = %url);
        let _guard = span.enter();

        let local_peer_id = PeerId::from(&self.key);
        let urn = RadUrn {
            path: uri::Path::empty(),
            ..url.urn.clone()
        };

        // Fetch the identity first
        let git_url = GitUrlRef::from_rad_url_ref(url.as_ref(), &local_peer_id);
        let meta: Entity<T, Draft> = self.fetch_id(git_url)?;

        // TODO: properly verify meta

        if meta.signatures().is_empty() {
            return Err(Error::UnsignedMetadata);
        }

        if meta.root_hash() != &url.urn.id {
            return Err(Error::RootHashMismatch {
                expected: url.urn.id.to_owned(),
                actual: meta.root_hash().to_owned(),
            });
        }

        self.track_signers(&meta)?;
        self.update_refs(&urn)?;
        self.fetch_repo(&urn, &url.authority)?;

        Ok(Repo {
            urn,
            storage: self.into(),
        })
    }

    pub fn fetch_repo(&self, urn: &RadUrn, from: &PeerId) -> Result<(), Error> {
        let span = tracing::info_span!("Storage::fetch", fetch.urn = %urn, fetch.from = %from);
        let _guard = span.enter();

        let namespace = &urn.id;

        let mut remote = {
            let local_peer = PeerId::from(&self.key);
            let url = GitUrlRef::from_rad_url_ref(urn.as_rad_url_ref(from), &local_peer);
            self.remote_anonymous(&url.to_string())
        }?;
        remote.connect(git2::Direction::Fetch)?;

        let rad_refs = self.rad_refs(urn)?;
        let tracked_trans = rad_refs.remotes.flatten().collect::<HashSet<&PeerId>>();

        // Fetch rad/refs of all known remotes
        {
            let refspecs =
                Refspec::rad_refs(namespace.clone(), from, tracked_trans.iter().cloned())
                    .map(|spec| spec.to_string())
                    .collect::<Vec<String>>();
            tracing::debug!(refspecs = ?refspecs, "Fetching rad/refs");
            remote.fetch(&refspecs, Some(&mut self.fetch_options()), None)?;
        }

        // Read the signed refs of all known remotes, and compare their `heads`
        // against the advertised refs. If signed and advertised branch head
        // match, non-fast-forwards are permitted. Otherwise, the branch is
        // skipped.
        {
            let remote_heads: HashMap<&str, git2::Oid> = remote
                .list()?
                .iter()
                .map(|rhead| (rhead.name(), rhead.oid()))
                .collect();

            let refspecs = Refspec::fetch_heads(
                namespace.clone(),
                remote_heads,
                tracked_trans.iter().cloned(),
                from,
                |peer| self.rad_refs_of(urn, peer),
                |peer| self.certifiers_of(urn, peer),
            )?
            .map(|spec| spec.to_string())
            .collect::<Vec<String>>();

            tracing::debug!(refspecs = ?refspecs, "Fetching refs/heads");
            remote.fetch(&refspecs, Some(&mut self.fetch_options()), None)?;
        }

        // At this point, the transitive tracking graph may have changed. Let's
        // update the refs, but don't recurse here for now (we could, if
        // we reload `self.refs()` and compare to the value we had
        // before fetching).
        self.update_refs(urn)
    }

    /// Get a [`surf::Browser`] for the project at `urn`. The `Browser` will be
    /// initialised with history found at the given `revision`.
    pub fn browser(&'_ self, urn: &RadUrn, revision: &str) -> Result<surf::Browser<'_>, Error> {
        let namespace = surf::Namespace::from(urn.id.to_string().as_str());
        // TODO(finto): Should the revision be the default branch of the project?
        // If so we need resolvers to fetch the project from the urn.
        Ok(surf::Browser::new_with_namespace(
            &self.backend,
            &namespace,
            revision,
        )?)
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
            .or_matches(is_not_found_err, || Ok(false))
    }

    pub fn has_urn(&self, urn: &RadUrn) -> Result<bool, Error> {
        let namespace = &urn.id;
        let branch = urn.path.deref_or_default();
        let branch = branch.strip_prefix("refs/").unwrap_or(branch);
        self.backend
            .find_reference(&format!("refs/namespaces/{}/refs/{}", namespace, branch))
            .map(|_| true)
            .or_matches(is_not_found_err, || Ok(false))
    }

    pub fn track(&self, urn: &RadUrn, peer: &PeerId) -> Result<(), Error> {
        let remote_name = tracking_remote_name(urn, peer);
        let url = GitUrlRef::from_rad_urn(&urn, &PeerId::from(&self.key), peer).to_string();

        tracing::debug!(
            urn = %urn,
            peer = %peer,
            "Storage::track"
        );

        self.backend
            .remote(&remote_name, &url)
            .map(|_| ())
            .or_matches(is_exists_err, || Ok(()))
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

    /// Read the current [`Refs`] from the repo state
    pub fn rad_refs(&self, urn: &RadUrn) -> Result<Refs, Error> {
        let span = tracing::debug_span!("Storage::rad_refs", urn = %urn);
        let _guard = span.enter();

        // Collect refs/heads (our branches) at their current state
        let heads = self.references_glob(urn, Some("refs/heads/*"))?;
        let heads: BTreeMap<String, Oid> = heads.map(|(name, oid)| (name, Oid(oid))).collect();

        tracing::debug!(heads = ?heads);

        // Get 1st degree tracked peers from the remotes configured in .git/config
        let tracked = self.tracked(urn)?;
        let mut remotes: HashMap<PeerId, HashMap<PeerId, HashSet<PeerId>>> =
            tracked.map(|peer| (peer, HashMap::new())).collect();

        tracing::debug!(remotes.bare = ?remotes);

        // For each of the 1st degree tracked peers, lookup their rad/refs (if any),
        // verify the signature, and add their [`Remotes`] to ours (minus the 3rd
        // degree)
        for (peer, tracked) in remotes.iter_mut() {
            match self.rad_refs_of(urn, peer.clone()) {
                Ok(refs) => *tracked = refs.remotes.cutoff(),
                Err(Error::NoSuchBranch(_)) | Err(Error::NoSuchBlob(_)) => {},
                Err(e) => return Err(e),
            }
        }

        tracing::debug!(remotes.verified = ?remotes);

        Ok(Refs {
            heads,
            remotes: remotes.into(),
        })
    }

    pub fn rad_refs_of(&self, urn: &RadUrn, peer: PeerId) -> Result<Refs, Error> {
        let signed = {
            let refs = Reference::rad_refs(urn.id.clone(), peer.clone());
            let blob = WithBlob::Tip {
                reference: &refs,
                file_name: "refs",
            }
            .get(&self)?;
            refs::Signed::from_json(blob.content(), &peer)
        }?;

        Ok(Refs::from(signed))
    }

    /// The set of all certifiers of the given identity, transitively
    pub fn certifiers(&self, urn: &RadUrn) -> Result<HashSet<RadUrn>, Error> {
        let mut refs = References::from_globs(
            &self,
            &[
                format!("refs/namespaces/{}/refs/rad/ids/*", &urn.id),
                format!("refs/namespaces/{}/refs/remotes/**/rad/ids/*", &urn.id),
            ],
        )?;
        let refnames = refs.names();
        Ok(urns_from_refs(refnames).collect())
    }

    pub fn commit(
        &self,
        urn: &RadUrn,
        branch: &str,
        msg: &str,
        tree: &git2::Tree,
        parents: &[&git2::Commit],
    ) -> Result<git2::Oid, Error> {
        let author = self.signature()?;
        let head = Reference::head(urn.id.clone(), None, branch);
        let oid = self.backend.commit(
            Some(&head.to_string()),
            &author,
            &author,
            msg,
            tree,
            parents,
        )?;

        self.update_refs(urn)?;

        Ok(oid)
    }

    pub fn references_glob<'a>(
        &'a self,
        urn: &RadUrn,
        globs: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<impl Iterator<Item = (String, git2::Oid)> + 'a, Error> {
        let namespace_prefix = format!("refs/namespaces/{}/", &urn.id);

        let refs = References::from_globs(
            &self,
            globs
                .into_iter()
                .map(|glob| format!("{}{}", namespace_prefix, glob.as_ref())),
        )?;

        Ok(refs.peeled().filter_map(move |(name, target)| {
            name.strip_prefix(&namespace_prefix)
                .map(|name| (name.to_owned(), target))
        }))
    }

    fn entity_blob<'a>(&'a self, commit: git2::Commit<'a>) -> Result<git2::Blob, Error> {
        blob(&self, commit.tree()?, "id")
    }

    pub fn first_parent(&self, commit_oid: git2::Oid) -> Option<git2::Commit> {
        self.backend
            .find_commit(commit_oid)
            .ok()
            .and_then(|commit| match commit.parents().len() {
                1 => commit.parents().next(),
                _ => None,
            })
    }

    pub fn linear_commit_history<'a>(
        &'a self,
        commit: git2::Commit<'a>,
    ) -> LinearHistoryCommits<'a> {
        LinearHistoryCommits(Some(commit))
    }

    fn entity_metadata_commit(&self, urn: &RadUrn) -> Result<git2::Commit, Error> {
        self.backend
            .find_reference(&Reference::rad_id(urn.id.clone()).to_string())
            .map_err(Error::Git)
            .and_then(|reference| {
                reference
                    .target()
                    .ok_or_else(|| Error::NoSuchUrn(urn.clone()))
            })
            .and_then(|oid| Ok(self.backend.find_commit(oid)?))
    }

    /// Get the [`Entity`] found at the provided [`RadUrn`].
    ///
    /// To use this, the caller will need to specify which `T` they wish to
    /// resolve to. This can be done one of two ways:
    ///
    /// * `let user: User = storage.entity(&urn);`
    /// * `let user = storage.entity::<UserInfo>(&urn);`
    ///
    /// # Errors
    ///   * If the entity resolution fails.
    pub fn entity<T>(&self, urn: &RadUrn) -> Result<Entity<T, Draft>, Error>
    where
        T: Serialize + DeserializeOwned + Clone + EntityInfoExt,
    {
        self.entity_metadata_commit(urn).and_then(|commit| {
            Ok(Entity::<T, Draft>::from_json_slice(
                self.entity_blob(commit)?.content(),
            )?)
        })
    }

    /// Get all the [`Entity`] data in this `Storage`.
    ///
    /// The caller has the choice to filter on the [`EntityInfo`], which is
    /// useful when the you want a list of a specific kind of `Entity`.
    pub fn entities(&self) -> Result<Vec<GenericDraftEntity>, Error> {
        let references = self.backend.references_glob("refs/namespaces/**/rad/id")?;

        let commits = references.filter_map(|reference| {
            let reference = reference.ok()?;
            let oid = reference.target()?;
            self.backend.find_commit(oid).ok()
        });

        // Fetch the entities generically
        commits
            .map(|commit| {
                self.entity_blob(commit)
                    .map(|blob| blob.content().to_vec())
                    .and_then(|content| Ok(GenericDraftEntity::from_json_slice(&content)?))
            })
            .collect()
    }

    pub fn all_metadata_heads<'a>(
        &'a self,
    ) -> Result<impl Iterator<Item = (RadUrn, git2::Commit)> + 'a, Error> {
        Ok(
            References::from_globs(&self, &["refs/namespaces/*/refs/rad/id"])?
                .peeled()
                .filter_map(move |(refname, oid)| {
                    urn_from_idref(&refname).and_then(|urn| {
                        self.backend
                            .find_commit(oid)
                            .map(|commit| (urn, commit))
                            .ok()
                    })
                }),
        )
    }

    pub fn certifiers_of(&self, urn: &RadUrn, peer: &PeerId) -> Result<HashSet<RadUrn>, Error> {
        let mut refs = References::from_globs(
            &self,
            &[format!(
                "refs/namespaces/{}/refs/remotes/{}/rad/ids/*",
                &urn.id, peer
            )],
        )?;
        let refnames = refs.names();
        Ok(urns_from_refs(refnames).collect())
    }

    // FIXME: decide if we want to require verified entities
    // FIXME: yes, we do want that
    fn fetch_id<T>(&self, url: GitUrlRef) -> Result<Entity<T, Draft>, Error>
    where
        T: Serialize + DeserializeOwned + Clone + EntityInfoExt,
    {
        tracing::debug!("Fetching id of {}", url);

        let namespace = url.repo.clone();
        let id_branch = Reference::rad_id(namespace.clone());
        let certifiers_glob = Reference::rad_ids_glob(namespace);

        // Map rad/id to rad/id (not remotes/X/rad/id) -- we need an owned
        // id, and the remote one is supposed to be valid regardless of the
        // peer we're cloning from. A resolver may later decide whether it's
        // up-to-date.
        let refspecs = [
            Refspec {
                remote: id_branch.clone(),
                local: id_branch.clone(),
                force: false,
            },
            Refspec {
                remote: certifiers_glob.clone(),
                local: certifiers_glob,
                force: false,
            },
        ]
        .iter()
        .map(|spec| spec.to_string())
        .collect::<Vec<String>>();

        {
            tracing::trace!(repo.clone.refspecs = ?refspecs);
            let mut remote = self.remote_anonymous(&url.to_string())?;
            remote.fetch(&refspecs, Some(&mut self.fetch_options()), None)?;
        }

        let entity: Entity<T, Draft> = {
            let blob = WithBlob::Init {
                reference: &id_branch,
                file_name: "id",
            }
            .get(&self)?;
            Entity::<T, Draft>::from_json_slice(blob.content())
        }?;

        Ok(entity)
    }

    fn commit_initial_meta<T>(&self, meta: &Entity<T, Draft>) -> Result<git2::Oid, Error>
    where
        T: Serialize + DeserializeOwned + Clone + EntityInfoExt,
    {
        let canonical_data = Cjson(meta).canonical_form()?;
        let blob = self.blob(&canonical_data)?;
        let tree = {
            let mut builder = self.treebuilder(None)?;
            builder.insert("id", blob, 0o100_644)?;
            let oid = builder.write()?;
            self.find_tree(oid)
        }?;
        let author = self.signature()?;

        let branch_name = Reference::rad_id(meta.urn().id);

        let oid = self.backend.commit(
            Some(&branch_name.to_string()),
            &author,
            &author,
            &format!("Initialised with identity {}", meta.root_hash()),
            &tree,
            &[],
        )?;

        tracing::debug!(
            repo.urn = %meta.urn(),
            repo.id.branch = %branch_name,
            repo.id.oid = %oid,
            "Initial metadata committed"
        );

        Ok(oid)
    }

    // FIXME: decide if we want to require verified entities
    // FIXME: yes, we want this
    fn track_signers<T>(&self, meta: &Entity<T, Draft>) -> Result<(), Error>
    where
        T: Serialize + DeserializeOwned + Clone + EntityInfoExt,
    {
        let span = tracing::debug_span!("Storage::track_signers", meta.urn = %meta.urn());
        let _guard = span.enter();

        let meta_urn = meta.urn();
        meta.signatures()
            .iter()
            .map(|(pk, sig)| {
                let peer_id = PeerId::from(pk.clone());
                match &sig.by {
                    Signatory::User(urn) => (peer_id, Some(urn)),
                    Signatory::OwnedKey => (peer_id, None),
                }
            })
            .try_for_each(|(peer, urn)| {
                tracing::debug!(
                    tracked.peer = %peer,
                    tracked.urn =
                        %urn.map(|urn| urn.to_string()).unwrap_or_else(|| "None".to_owned()),
                    "Tracking signer of {}",
                    meta.urn()
                );

                // Track the signer's version of this repo (if any)
                self.track(&meta_urn, &peer)?;
                // Track the signer's version of the identity she used for
                // signing (if any)
                if let Some(urn) = urn {
                    self.track(urn, &peer)?;
                }

                Ok(())
            })
    }

    fn update_refs(&self, urn: &RadUrn) -> Result<(), Error> {
        let span = tracing::debug_span!("Storage::update_refs");
        let _guard = span.enter();

        let refsig_canonical = self
            .rad_refs(urn)?
            .sign(&self.key)
            .and_then(|signed| Cjson(signed).canonical_form())?;

        let rad_refs_ref = Reference::rad_refs(urn.id.clone(), None).to_string();

        let parent: Option<git2::Commit> = self
            .find_reference(&rad_refs_ref)
            .and_then(|refs| refs.peel_to_commit().map(Some))
            .or_matches::<Error, _, _>(is_not_found_err, || Ok(None))?;
        let tree = {
            let blob = self.blob(&refsig_canonical)?;
            let mut builder = self.treebuilder(None)?;

            builder.insert("refs", blob, 0o100_644)?;
            let oid = builder.write()?;

            self.find_tree(oid)
        }?;

        // Don't create a new commit if it would be the same tree as the parent
        if let Some(ref parent) = parent {
            if parent.tree()?.id() == tree.id() {
                return Ok(());
            }
        }

        let author = self.signature()?;
        self.backend.commit(
            Some(&rad_refs_ref),
            &author,
            &author,
            "",
            &tree,
            &parent.iter().collect::<Vec<&git2::Commit>>(),
        )?;

        Ok(())
    }

    // TODO: allow users to supply callbacks
    fn fetch_options(&'_ self) -> git2::FetchOptions<'_> {
        let mut cbs = git2::RemoteCallbacks::new();
        cbs.sideband_progress(|prog| {
            tracing::trace!("{}", unsafe { std::str::from_utf8_unchecked(prog) });
            true
        })
        .update_tips(|name, old, new| {
            tracing::debug!("{}: {} -> {}", name, old, new);
            true
        });

        let mut fos = git2::FetchOptions::new();
        fos.prune(git2::FetchPrune::Off)
            .update_fetchhead(true)
            .download_tags(git2::AutotagOption::None)
            .remote_callbacks(cbs);

        fos
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
                    .or_matches(is_not_found_err, || Err(Error::NoSuchBranch(ref_name)))?;
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

fn urn_from_idref(refname: &str) -> Option<RadUrn> {
    refname
        .strip_suffix("/refs/rad/id")
        .and_then(|namespace_root| {
            namespace_root
                .split('/')
                .next_back()
                .and_then(|namespace| Hash::from_str(namespace).ok())
                .map(|hash| RadUrn::new(hash, Protocol::Git, Path::empty()))
        })
}

fn urn_from_ref(refname: &str) -> Option<RadUrn> {
    refname
        .split('/')
        .next_back()
        .and_then(|urn| urn.parse().ok())
}

fn urns_from_refs<'a, E>(
    refs: impl Iterator<Item = Result<&'a str, E>> + 'a,
) -> impl Iterator<Item = RadUrn> + 'a {
    refs.filter_map(|refname| refname.ok().and_then(urn_from_ref))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta::{
        entity::{data::EntityInfo, Draft, GenericDraftEntity, Resolver},
        Project,
        User,
    };
    use async_trait::async_trait;
    use futures_await_test::async_test;

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
    fn test_idempotent_tracking() {
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

        // Attempting to track again does not fail
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

    #[async_test]
    async fn test_specific_entity_resolution() -> Result<(), Error> {
        let tmp = tempdir().unwrap();
        let paths = Paths::from_root(tmp).unwrap();
        let user_key = SecretKey::new();
        let store = Storage::init(&paths, user_key.clone()).unwrap();

        // Setup and verify owner
        let mut owner = User::<Draft>::create("radicle".to_owned(), user_key.public()).unwrap();
        owner.sign_owned(&user_key).unwrap();
        let user_resolver = DummyUserResolver(owner.clone());
        let verified_owner = owner
            .clone()
            .check_history_status(&user_resolver, &user_resolver)
            .await
            .unwrap();

        // Store the owner in the monorepo
        let _repo = store.create_repo(&owner)?;

        // Assert we can fetch it back
        assert_eq!(owner, store.entity(&owner.urn())?);

        // Setup a project for the owner
        let mut project_banana =
            Project::<Draft>::create("banana".to_owned(), owner.urn()).unwrap();
        project_banana
            .sign_by_user(&user_key, &verified_owner)
            .unwrap();

        // Store the project in the monorepo
        let _repo = store.create_repo(&project_banana)?;

        // And assert we can get it back
        assert_eq!(project_banana, store.entity(&project_banana.urn())?);

        // Assert that getting the wrong entity provides the correct error
        let is_it_a_user: Result<User<Draft>, Error> = store.entity(&project_banana.urn());
        assert!(is_it_a_user.is_err());

        let is_it_a_project: Result<Project<Draft>, Error> = store.entity(&owner.urn());
        assert!(is_it_a_project.is_err());

        Ok(())
    }

    #[async_test]
    async fn test_listing_entities() -> Result<(), Error> {
        let tmp = tempdir().unwrap();
        let paths = Paths::from_root(tmp).unwrap();
        let key = SecretKey::new();
        let storage = Storage::open(&paths, key.clone())?;

        // Setup and verify owner
        let mut owner = User::<Draft>::create("radicle".to_owned(), key.public()).unwrap();
        owner.sign_owned(&key)?;
        let user_resolver = DummyUserResolver(owner.clone());
        let verified_owner = owner
            .clone()
            .check_history_status(&user_resolver, &user_resolver)
            .await
            .unwrap();

        let _repo = storage.create_repo(&owner)?;

        let mut project_banana = Project::<Draft>::create("banana".to_owned(), owner.urn())?;
        project_banana.sign_by_user(&key, &verified_owner)?;

        let _repo = storage.create_repo(&project_banana)?;

        let mut project_pineapple = Project::<Draft>::create("pineapple".to_owned(), owner.urn())?;
        project_pineapple.sign_by_user(&key, &verified_owner)?;

        let _repo = storage.create_repo(&project_pineapple)?;

        let mut projects = storage
            .entities()?
            .into_iter()
            .filter_map(|entity| {
                entity.try_map(|info| match info {
                    EntityInfo::Project(info) => Some(info),
                    _ => None,
                })
            })
            .collect::<Vec<_>>();

        projects.sort_by_key(|project| {
            let name = project.name();
            name.to_owned()
        });

        assert_eq!(
            projects,
            vec![project_banana.clone(), project_pineapple.clone()]
        );

        let users = storage
            .entities()?
            .into_iter()
            .filter_map(|entity| {
                entity.try_map(|info| match info {
                    EntityInfo::User(info) => Some(info),
                    _ => None,
                })
            })
            .collect::<Vec<_>>();

        assert_eq!(users, vec![owner.clone()]);

        let mut all_entities = storage.entities()?;

        all_entities.sort_by_key(|entity| {
            let name = entity.name();
            name.to_owned()
        });

        assert_eq!(
            all_entities,
            vec![
                project_banana.map(EntityInfo::Project),
                project_pineapple.map(EntityInfo::Project),
                owner.map(EntityInfo::User),
            ]
        );
        Ok(())
    }

    struct DummyUserResolver(User<Draft>);
    #[async_trait]
    impl Resolver<User<Draft>> for DummyUserResolver {
        async fn resolve(&self, _uri: &RadUrn) -> Result<User<Draft>, entity::Error> {
            Ok(self.0.clone())
        }
        async fn resolve_revision(
            &self,
            _uri: &RadUrn,
            _revision: u64,
        ) -> Result<User<Draft>, entity::Error> {
            Ok(self.0.clone())
        }
    }

    #[async_test]
    async fn test_all_metadata_heads() {
        let tmp = tempdir().unwrap();
        let paths = Paths::from_root(tmp).unwrap();
        let user_key = SecretKey::new();
        let store = Storage::init(&paths, user_key.clone()).unwrap();

        // Create signed and verified user
        let mut user = User::<Draft>::create("user".to_owned(), user_key.public()).unwrap();
        user.sign_owned(&user_key).unwrap();
        let user_resolver = DummyUserResolver(user.clone());
        let verified_user = user
            .clone()
            .check_history_status(&user_resolver, &user_resolver)
            .await
            .unwrap();

        // Create and sign two projects
        let mut project_foo = Project::<Draft>::create("foo".to_owned(), user.urn()).unwrap();
        let mut project_bar = Project::<Draft>::create("bar".to_owned(), user.urn()).unwrap();
        project_foo.sign_by_user(&user_key, &verified_user).unwrap();
        project_bar.sign_by_user(&user_key, &verified_user).unwrap();

        // Store the three entities in their respective namespaces
        store.create_repo(&user).unwrap();
        store.create_repo(&project_foo).unwrap();
        store.create_repo(&project_bar).unwrap();

        let mut ids = HashSet::new();
        let mut urns = HashMap::new();
        ids.insert(user.hash());
        ids.insert(project_foo.hash());
        ids.insert(project_bar.hash());

        // Iterate ove all namespaces
        for (urn, commit) in store.all_metadata_heads().unwrap() {
            // Check that we found one of our IDs
            let id = &urn.id;
            assert!(ids.contains(id));

            // Check that we can use the URN to find the same commit
            let commit_from_urn = store.entity_metadata_commit(&urn).unwrap();
            assert_eq!(commit_from_urn.id(), commit.id());

            // Bookkeeping for more tests
            ids.remove(id);
            urns.insert(id.to_owned(), urn);
        }

        // Check that we found all the entities that we saved
        assert!(ids.is_empty());

        // Pull out user blob and deserialize
        assert_eq!(
            user,
            User::<Draft>::from_json_slice(
                store
                    .entity_blob(store.entity_metadata_commit(&user.urn()).unwrap())
                    .unwrap()
                    .content()
            )
            .unwrap()
        );
        let generic_user: GenericDraftEntity = store.entity(&user.urn()).unwrap();
        assert_eq!(generic_user.kind(), user.kind());
        assert_eq!(generic_user.hash(), user.hash());

        // Pull out foo blob and deserialize
        assert_eq!(
            project_foo,
            Project::<Draft>::from_json_slice(
                store
                    .entity_blob(store.entity_metadata_commit(&project_foo.urn()).unwrap())
                    .unwrap()
                    .content()
            )
            .unwrap()
        );
        let generic_foo: GenericDraftEntity = store.entity(&project_foo.urn()).unwrap();
        assert_eq!(generic_foo.kind(), project_foo.kind());
        assert_eq!(generic_foo.hash(), project_foo.hash());

        // Check user commit history length
        let user_history =
            store.linear_commit_history(store.entity_metadata_commit(&user.urn()).unwrap());
        let user_commits: Vec<git2::Commit> = user_history.collect();
        assert_eq!(user_commits.len(), 1);
    }

    #[test]
    fn test_open_or_init() {
        let tmp = tempdir().unwrap();
        let paths = Paths::from_root(tmp).unwrap();
        let key = SecretKey::new();
        let store = Storage::open(&paths, key);
        if let Err(err) = store {
            assert!(false, "failed to open Storage: {:?}", err)
        };
    }
}
