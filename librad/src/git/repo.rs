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
    sync::MutexGuard,
};

use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

use crate::{
    git::{
        ext::{is_not_found_err, Oid},
        refs::{self, Refs},
        storage::{self, Side, Storage, WithBlob},
        types::{Namespace, Reference, RefsCategory, Refspec},
        url::GitUrlRef,
    },
    hash::Hash,
    internal::canonical::{Cjson, CjsonError},
    keys::SecretKey,
    meta::entity::{
        self,
        data::{EntityBuilder, EntityData},
        Entity,
        Signatory,
    },
    peer::PeerId,
    uri::{self, RadUrl, RadUrn},
};

pub use storage::Tracked;

#[derive(Debug, Error)]
pub enum Error {
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
    Storage(#[from] storage::Error),

    #[error(transparent)]
    Git(#[from] git2::Error),
}

pub struct Repo {
    urn: RadUrn,
    storage: Storage,
}

impl Repo {
    pub fn urn(&self) -> RadUrn {
        self.urn.clone()
    }

    pub fn locked(&mut self) -> Locked {
        Locked {
            urn: &self.urn,
            key: &self.storage.key,
            git: self.storage.lock(),
        }
    }

    pub fn create<T>(storage: Storage, meta: &Entity<T>) -> Result<Self, Error>
    where
        T: Serialize + DeserializeOwned + Clone + Default,
        EntityData<T>: EntityBuilder,
    {
        let span = tracing::info_span!("Repo::create");
        let _guard = span.enter();

        // FIXME: properly verify meta

        if meta.signatures().is_empty() {
            return Err(Error::UnsignedMetadata);
        }

        // FIXME: certifier identities must exist, or be supplied

        let mut this = Self {
            urn: RadUrn::new(
                meta.root_hash().to_owned(),
                uri::Protocol::Git,
                uri::Path::empty(),
            ),
            storage,
        };
        this.locked().commit_initial_meta(meta)?;
        this.track_signers(&meta)?;
        this.locked().update_refs()?;

        Ok(this)
    }

    pub fn open(storage: Storage, urn: RadUrn) -> Result<Self, Error> {
        {
            let id_ref = Reference {
                namespace: urn.id.clone(),
                remote: None,
                category: RefsCategory::Rad,
                name: "id".to_owned(),
            };
            if !storage.has_ref(&id_ref)? {
                return Err(Error::NoSuchUrn(urn));
            }
        }

        Ok(Self {
            urn: RadUrn {
                path: uri::Path::empty(),
                ..urn
            },
            storage,
        })
    }

    pub fn clone<T>(storage: Storage, url: RadUrl) -> Result<Self, Error>
    where
        T: Serialize + DeserializeOwned + Clone + Default,
        EntityData<T>: EntityBuilder,
    {
        let span = tracing::info_span!("Repo::clone", repo.url = %url);
        let _guard = span.enter();

        let local_peer_id = PeerId::from(&storage.key);
        let mut this = Self {
            urn: RadUrn {
                path: uri::Path::empty(),
                ..url.urn.clone()
            },
            storage,
        };

        // Fetch the identity first
        let git_url = GitUrlRef::from_rad_url_ref(url.as_ref(), &local_peer_id);
        let meta = this.locked().fetch_id(git_url)?;

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

        this.track_signers(&meta)?;
        this.locked().update_refs()?;
        this.fetch(&url.authority)?;

        Ok(this)
    }

    pub fn fetch(&mut self, from: &PeerId) -> Result<(), Error> {
        let span = tracing::info_span!("Repo::fetch", repo.fetch.from = %from);
        let _guard = span.enter();
        self.locked().fetch(from)
    }

    pub fn track(&self, peer: &PeerId) -> Result<(), Error> {
        self.storage.track(&self.urn, peer)?;
        Ok(())
    }

    fn track_signers<T>(&self, meta: &Entity<T>) -> Result<(), Error>
    where
        T: Serialize + DeserializeOwned + Clone + Default,
    {
        let span = tracing::debug_span!("Repo::track_signers", meta.urn = %meta.urn());
        let _guard = span.enter();

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
                self.track(&peer)?;
                // Track the signer's version of the identity she used for
                // signing (if any)
                if let Some(urn) = urn {
                    self.storage.track(urn, &peer)?;
                }

                Ok(())
            })
    }

    pub fn untrack(&self, peer: &PeerId) -> Result<(), Error> {
        self.storage.untrack(&self.urn, peer)?;
        Ok(())
    }

    /// Retrieve all _directly_ tracked peers
    ///
    /// To retrieve the transitively tracked peers, use [`rad_refs`] and inspect
    /// the `remotes`.
    pub fn tracked(&mut self) -> Result<Tracked, Error> {
        let tracked = self.storage.tracked(&self.urn)?;
        Ok(tracked)
    }

    /// Read the current [`Refs`] from the repo state
    pub fn rad_refs(&mut self) -> Result<Refs, Error> {
        self.locked().rad_refs()
    }

    /// The set of all certifiers of this repo's identity, transitively
    pub fn certifiers(&mut self) -> Result<HashSet<RadUrn>, Error> {
        self.locked().certifiers()
    }
}

pub struct Locked<'a> {
    urn: &'a RadUrn,
    key: &'a SecretKey,
    git: MutexGuard<'a, git2::Repository>,
}

impl<'a> Locked<'a> {
    pub fn namespace(&self) -> Namespace {
        self.urn.id.clone()
    }

    pub fn index(&self) -> Result<git2::Index, Error> {
        let idx = self.git.index()?;
        Ok(idx)
    }

    pub fn find_tree(&self, oid: git2::Oid) -> Result<git2::Tree, Error> {
        let tree = self.git.find_tree(oid)?;
        Ok(tree)
    }

    pub fn blob(&self, data: &[u8]) -> Result<git2::Oid, Error> {
        let oid = self.git.blob(data)?;
        Ok(oid)
    }

    pub fn find_blob(&self, oid: git2::Oid) -> Result<git2::Blob, Error> {
        let blob = self.git.find_blob(oid)?;
        Ok(blob)
    }

    pub fn commit(
        &self,
        branch: &str,
        msg: &str,
        tree: &git2::Tree,
        parents: &[&git2::Commit],
    ) -> Result<git2::Oid, Error> {
        let author = self.git.signature()?;
        let head = Reference {
            namespace: self.namespace(),
            remote: None,
            category: RefsCategory::Heads,
            name: branch.to_owned(),
        };
        let oid = self.git.commit(
            Some(&head.to_string()),
            &author,
            &author,
            msg,
            tree,
            parents,
        )?;

        self.update_refs()?;

        Ok(oid)
    }

    pub fn find_commit(&self, oid: git2::Oid) -> Result<git2::Commit, Error> {
        let commit = self.git.find_commit(oid)?;
        Ok(commit)
    }

    pub fn references_glob(&self, glob: &str) -> Result<Vec<(String, git2::Oid)>, Error> {
        let prefix = format!("refs/namespaces/{}/", &self.urn.id);
        let refs = self.git.references_glob(&format!("{}{}", &prefix, glob))?;
        let iter = refs.filter_map(|reference| {
            if let Ok(head) = reference {
                if let (Some(name), Some(target)) = (
                    head.name().and_then(|name| name.strip_prefix(&prefix)),
                    head.target(),
                ) {
                    Some((name.to_owned(), target.to_owned()))
                } else {
                    None
                }
            } else {
                None
            }
        });

        Ok(iter.collect())
    }

    fn commit_initial_meta<T>(&self, meta: &Entity<T>) -> Result<git2::Oid, Error>
    where
        T: Serialize + DeserializeOwned + Clone + Default,
        EntityData<T>: EntityBuilder,
    {
        let canonical_data = Cjson(meta).canonical_form()?;
        let blob = self.git.blob(&canonical_data)?;
        let tree = {
            let mut builder = self.git.treebuilder(None)?;
            builder.insert("id", blob, 0o100_644)?;
            let oid = builder.write()?;
            self.git.find_tree(oid)
        }?;
        let author = self.git.signature()?;

        let branch_name = Reference::rad_id(self.namespace());

        let oid = self.git.commit(
            Some(&branch_name.to_string()),
            &author,
            &author,
            &format!("Initialised with identity {}", meta.root_hash()),
            &tree,
            &[],
        )?;

        tracing::debug!(
            repo.urn = %self.urn,
            repo.id.branch = %branch_name,
            repo.id.oid = %oid,
            "Initial metadata committed"
        );

        Ok(oid)
    }

    fn fetch_id<T>(&self, url: GitUrlRef) -> Result<Entity<T>, Error>
    where
        T: Serialize + DeserializeOwned + Clone + Default,
        EntityData<T>: EntityBuilder,
    {
        tracing::debug!("Fetching id of {}", url);

        let id_branch = Reference::rad_id(self.namespace());
        let certifiers_glob = Reference::rad_ids_glob(self.namespace());

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
            let mut remote = self.git.remote_anonymous(&url.to_string())?;
            remote.fetch(&refspecs, None, None)?;
        }

        let entity: Entity<T> = {
            let blob = WithBlob {
                reference: &id_branch,
                file_name: "id",
                side: Side::First,
            }
            .get(&self.git)?;
            Entity::from_json_slice(blob.content())
        }?;

        Ok(entity)
    }

    fn rad_refs(&self) -> Result<Refs, Error> {
        let span = tracing::debug_span!("Repo::Locked::rad_refs", urn = %self.urn);
        let _guard = span.enter();

        // Collect refs/heads (our branches) at their current state
        let heads = self.references_glob("refs/heads/*")?;
        let heads: BTreeMap<String, Oid> = heads
            .into_iter()
            .map(|(name, oid)| (name, Oid(oid)))
            .collect();

        tracing::debug!(heads = ?heads);

        // Get 1st degree tracked peers from the remotes configured in .git/config
        let tracked = self.tracked()?;
        let mut remotes: HashMap<PeerId, HashMap<PeerId, HashSet<PeerId>>> =
            tracked.map(|peer| (peer, HashMap::new())).collect();

        tracing::debug!(remotes.bare = ?remotes);

        // For each of the 1st degree tracked peers, lookup their rad/refs (if any),
        // verify the signature, and add their [`Remotes`] to ours (minus the 3rd
        // degree)
        for (peer, tracked) in remotes.iter_mut() {
            match self.rad_refs_of(peer.clone()) {
                Ok(refs) => *tracked = refs.remotes.cutoff(),
                Err(Error::Storage(storage::Error::NoSuchBranch(_)))
                | Err(Error::Storage(storage::Error::NoSuchBlob(_))) => {},
                Err(e) => return Err(e),
            }
        }

        tracing::debug!(remotes.verified = ?remotes);

        Ok(Refs {
            heads,
            remotes: remotes.into(),
        })
    }

    fn tracked(&self) -> Result<Tracked, Error> {
        let remotes = self.git.remotes()?;
        Ok(Tracked::new(remotes, &self.urn))
    }

    fn rad_refs_of(&self, peer: PeerId) -> Result<Refs, Error> {
        let signed = {
            let refs = Reference {
                namespace: self.namespace(),
                remote: Some(peer.clone()),
                category: RefsCategory::Rad,
                name: "refs".to_owned(),
            };
            let blob = WithBlob {
                reference: &refs,
                file_name: "refs",
                side: Side::Tip,
            }
            .get(&self.git)?;
            refs::Signed::from_json(blob.content(), &peer)
        }?;

        Ok(Refs::from(signed))
    }

    fn update_refs(&self) -> Result<(), Error> {
        let span = tracing::debug_span!("Repo::update_refs");
        let _guard = span.enter();

        let refsig_canonical = self
            .rad_refs()?
            .sign(self.key)
            .and_then(|signed| Cjson(signed).canonical_form())?;

        let rad_refs_ref = Reference::rad_refs(self.namespace(), None).to_string();

        let parent: Option<git2::Commit> = self
            .git
            .find_reference(&rad_refs_ref)
            .and_then(|refs| refs.peel_to_commit().map(Some))
            .or_else(|e| {
                if is_not_found_err(&e) {
                    Ok(None)
                } else {
                    Err(e)
                }
            })?;
        let tree = {
            let blob = self.git.blob(&refsig_canonical)?;
            let mut builder = self.git.treebuilder(None)?;

            builder.insert("refs", blob, 0o100_644)?;
            let oid = builder.write()?;

            self.git.find_tree(oid)
        }?;

        // Don't create a new commit if it would be the same tree as the parent
        if let Some(ref parent) = parent {
            if parent.tree()?.id() == tree.id() {
                return Ok(());
            }
        }

        let author = self.git.signature()?;
        self.git.commit(
            Some(&rad_refs_ref),
            &author,
            &author,
            "",
            &tree,
            &parent.iter().collect::<Vec<&git2::Commit>>(),
        )?;

        Ok(())
    }

    fn fetch(&self, from: &PeerId) -> Result<(), Error> {
        let namespace = &self.urn.id;

        let mut remote = {
            let local_peer = PeerId::from(self.key);
            let url = GitUrlRef::from_rad_url_ref(self.urn.as_rad_url_ref(from), &local_peer);
            self.git.remote_anonymous(&url.to_string())
        }?;
        remote.connect(git2::Direction::Fetch)?;

        let rad_refs = self.rad_refs()?;
        let tracked_trans = rad_refs.remotes.flatten().collect::<HashSet<&PeerId>>();

        // Fetch rad/refs of all known remotes
        {
            let refspecs =
                Refspec::rad_refs(namespace.clone(), from, tracked_trans.iter().cloned())
                    .map(|spec| spec.to_string())
                    .collect::<Vec<String>>();
            tracing::debug!(refspecs = ?refspecs, "Fetching rad/refs");
            remote.fetch(&refspecs, None, None)?;
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
                |peer| self.rad_refs_of(peer),
                |peer| self.certifiers_of(peer),
            )?
            .map(|spec| spec.to_string())
            .collect::<Vec<String>>();

            tracing::debug!(refspecs = ?refspecs, "Fetching refs/heads");
            remote.fetch(&refspecs, None, None)?;
        }

        // At this point, the transitive tracking graph may have changed. Let's
        // update the refs, but don't recurse here for now (we could, if
        // we reload `self.refs()` and compare to the value we had
        // before fetching).
        self.update_refs()
    }

    fn certifiers(&self) -> Result<HashSet<RadUrn>, Error> {
        let mut refs = self
            .git
            .references_glob(&format!("refs/namespaces/{}/**/rad/ids/*", &self.urn.id))?;
        let refnames = refs.names();
        Ok(urns_from_refs(refnames).collect())
    }

    fn certifiers_of(&self, peer: &PeerId) -> Result<HashSet<RadUrn>, Error> {
        let mut refs = self.git.references_glob(&format!(
            "refs/namespaces/{}/refs/remotes/{}/rad/ids/*",
            &self.urn.id, peer
        ))?;
        let refnames = refs.names();
        Ok(urns_from_refs(refnames).collect())
    }
}

fn urns_from_refs<'a, E>(
    refs: impl Iterator<Item = Result<&'a str, E>> + 'a,
) -> impl Iterator<Item = RadUrn> + 'a {
    refs.filter_map(|refname| {
        refname
            .ok()
            .and_then(|name| name.split('/').next_back())
            .and_then(|urn| urn.parse().ok())
    })
}
