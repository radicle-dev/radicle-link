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
    ops::Range,
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
    meta::entity::{
        self,
        data::{EntityBuilder, EntityData},
        Entity,
        Signatory,
    },
    peer::PeerId,
    uri::{self, RadUrl, RadUrn},
};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Unknown repo {0}")]
    NoSuchUrn(RadUrn),

    #[error(
        "Identity root hash doesn't match resolved URL. Expected {expected}, actual: {actual}"
    )]
    RootHashMismatch { expected: Hash, actual: Hash },

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
    local_peer_id: PeerId,
    storage: Storage,
}

impl Repo {
    pub fn urn(&self) -> RadUrn {
        self.urn.clone()
    }

    fn namespace(&self) -> Namespace {
        self.urn().id
    }

    pub fn create<T>(storage: Storage, meta: &Entity<T>) -> Result<Self, Error>
    where
        T: Serialize + DeserializeOwned + Clone + Default,
    {
        // FIXME: entity must be valid
        // FIXME: certifier identities must exist, or be supplied

        let namespace = meta.root_hash().to_owned();

        println!("create id");
        {
            let git = storage.backend();
            let canonical_data: Vec<u8> = meta.to_data().canonical_data()?;
            let blob = git.blob(&canonical_data)?;
            let tree = {
                let mut builder = git.treebuilder(None)?;
                builder.insert("id", blob, 0o100_644)?;
                let oid = builder.write()?;
                git.find_tree(oid)
            }?;
            let author = git.signature()?;

            let branch_name = id_branch(namespace);
            println!("branch: {}", branch_name);
            git.commit(
                Some(&branch_name.to_string()),
                &author,
                &author,
                &format!("Initialised with identity {}", meta.root_hash()),
                &tree,
                &[],
            )?;
        }

        let this = Self {
            urn: RadUrn::new(
                meta.root_hash().to_owned(),
                uri::Protocol::Git,
                uri::Path::empty(),
            ),
            local_peer_id: PeerId::from(&storage.key),
            storage,
        };
        println!("track signers");
        this.track_signers(&meta)?;
        println!("update_refs");
        this.update_refs()?;
        println!("created");

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
            local_peer_id: PeerId::from(&storage.key),
            storage,
        })
    }

    pub fn clone<T>(storage: Storage, url: RadUrl) -> Result<Self, Error>
    where
        T: Serialize + DeserializeOwned + Clone + Default,
        EntityData<T>: EntityBuilder,
    {
        let local_peer_id = PeerId::from(&storage.key);

        // Fetch the identity first
        let git_url = GitUrlRef::from_rad_url_ref(url.as_ref(), &local_peer_id);
        let entity: Entity<T> = {
            let id_branch = id_branch(url.urn.id.clone());
            let certifiers_glob = certifiers_glob(url.urn.id.clone());

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

            println!("clone id refspecs: {:?}", refspecs);

            {
                let git = storage.backend();
                let mut remote = git.remote_anonymous(&git_url.to_string())?;
                remote.fetch(&refspecs, None, None)?;
            }

            {
                let git = storage.backend();
                println!("references after id fetch:");
                for refname in git.references()?.names() {
                    let refname = refname?;
                    println!("{}", refname);
                }

                let blob = WithBlob {
                    reference: &id_branch,
                    file_name: "id",
                    side: Side::First,
                }
                .get(&git)?;
                Entity::from_json_slice(blob.content())
            }
        }?;

        // TODO: properly verify entity

        if entity.root_hash() != &url.urn.id {
            return Err(Error::RootHashMismatch {
                expected: url.urn.id.to_owned(),
                actual: entity.root_hash().to_owned(),
            });
        }

        let this = Self {
            urn: RadUrn {
                path: uri::Path::empty(),
                ..url.urn
            },
            local_peer_id,
            storage,
        };
        this.track_signers(&entity)?;
        this.fetch(&url.authority)?;

        Ok(this)
    }

    pub fn fetch(&self, from: &PeerId) -> Result<(), Error> {
        // Lock scope for `git`
        {
            let git = self.storage.backend();
            let namespace = &self.urn.id;

            let mut remote = git.remote_anonymous(
                &GitUrlRef::from_rad_url_ref(self.urn.as_rad_url_ref(from), &self.local_peer_id)
                    .to_string(),
            )?;
            remote.connect(git2::Direction::Fetch)?;

            let rad_refs = self.rad_refs()?;
            let tracked_trans = rad_refs.remotes.flatten().collect::<HashSet<&PeerId>>();

            // Fetch rad/refs of all known remotes
            remote.fetch(
                &rad_refs_specs(namespace.clone(), from, tracked_trans.iter().cloned())
                    .map(|spec| spec.to_string())
                    .collect::<Vec<String>>(),
                None,
                None,
            )?;

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

                let refspecs = fetch_specs(
                    namespace.clone(),
                    remote_heads,
                    tracked_trans.iter().cloned(),
                    from,
                    |peer| self.rad_refs_of(peer),
                    |peer| self.certifiers_of(peer),
                )?
                .map(|spec| spec.to_string())
                .collect::<Vec<String>>();

                remote.fetch(&refspecs, None, None)?;
            }
        }

        // At this point, the transitive tracking graph may have changed. Let's
        // update the refs, but don't recurse here for now (we could, if
        // we reload `self.refs()` and compare to the value we had
        // before fetching).
        self.update_refs()
    }

    pub fn track(&self, peer: PeerId) -> Result<(), Error> {
        self.storage.track(&self.urn, peer).map_err(|e| e.into())
    }

    fn track_signers<T>(&self, meta: &Entity<T>) -> Result<(), Error>
    where
        T: Serialize + DeserializeOwned + Clone + Default,
    {
        meta.signatures()
            .iter()
            .filter_map(|(pk, sig)| match &sig.by {
                Signatory::User(urn) => Some((PeerId::from(pk.clone()), urn)),
                Signatory::OwnedKey => None,
            })
            .try_for_each(|(peer, urn)| {
                // Track the signer's version of this repo (if any)
                self.track(peer.clone())?;
                // Track the signer's version of the identity she used for signing
                self.storage.track(urn, peer).map_err(|e| e.into())
            })
    }

    pub fn untrack(&self, peer: PeerId) -> Result<(), Error> {
        self.storage.untrack(&self.urn, peer).map_err(|e| e.into())
    }

    /// Retrieve all _directly_ tracked peers
    ///
    /// To retrieve the transitively tracked peers, use [`rad_refs`] and inspect
    /// the `remotes`.
    pub fn tracked(&self) -> Result<Tracked, Error> {
        let prefix = format!("{}/", &self.urn.id);
        let remotes = self.storage.backend().remotes()?;
        let range = 0..remotes.len();
        Ok(Tracked {
            remotes,
            range,
            prefix,
        })
    }

    /// Read the current [`Refs`] from the repo state
    pub fn rad_refs(&self) -> Result<Refs, Error> {
        // Collect refs/heads (our branches) at their current state
        let mut heads = BTreeMap::new();
        {
            let git = self.storage.backend();
            let ns = format!("refs/namespaces/{}/", self.urn.id);
            let local_heads = git.references_glob(&format!("{}refs/heads/*", ns))?;
            for res in local_heads {
                let head = res?;
                if let (Some(name), Some(target)) = (
                    head.name().and_then(|name| name.strip_prefix(&ns)),
                    head.target(),
                ) {
                    heads.insert(name.to_owned(), Oid(target.to_owned()));
                }
            }
        }

        // Get 1st degree tracked peers from the remotes configured in .git/config
        let tracked = self.tracked()?;
        let mut remotes: HashMap<PeerId, HashMap<PeerId, HashSet<PeerId>>> =
            tracked.map(|peer| (peer, HashMap::new())).collect();

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

        Ok(Refs {
            heads,
            remotes: remotes.into(),
        })
    }

    fn rad_refs_of(&self, peer: PeerId) -> Result<Refs, Error> {
        let signed = {
            let git = self.storage.backend();
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
            .get(&git)?;
            refs::Signed::from_json(blob.content(), &peer)
        }?;

        Ok(Refs::from(signed))
    }

    fn update_refs(&self) -> Result<(), Error> {
        let rad_refs = Reference {
            namespace: self.namespace(),
            remote: None,
            category: RefsCategory::Rad,
            name: "refs".to_owned(),
        };

        let refsig_canonical = self
            .rad_refs()?
            .sign(&self.storage.key)
            .and_then(|signed| Cjson(signed).canonical_form())?;

        let git = self.storage.backend();
        {
            let parent: Option<git2::Commit> = git
                .find_reference(&rad_refs.to_string())
                .and_then(|refs| refs.peel_to_commit().map(Some))
                .or_else(|e| {
                    if is_not_found_err(&e) {
                        Ok(None)
                    } else {
                        Err(e)
                    }
                })?;
            let tree = {
                let blob = git.blob(&refsig_canonical)?;
                let mut builder = git.treebuilder(None)?;

                builder.insert("refs", blob, 0o100_644)?;
                let oid = builder.write()?;

                git.find_tree(oid)
            }?;

            // Don't create a new commit if it would be the same tree as the parent
            if let Some(ref parent) = parent {
                if parent.tree()?.id() == tree.id() {
                    return Ok(());
                }
            }

            let author = git.signature()?;
            git.commit(
                Some(&rad_refs.to_string()),
                &author,
                &author,
                "",
                &tree,
                &parent.iter().collect::<Vec<&git2::Commit>>(),
            )?;
        }

        Ok(())
    }

    /// The set of all certifiers of this repo's identity, transitively
    pub fn certifiers(&self) -> Result<HashSet<RadUrn>, Error> {
        let git = self.storage.backend();
        let mut refs =
            git.references_glob(&format!("refs/namespaces/{}/**/rad/ids/*", &self.urn.id))?;
        let refnames = refs.names();
        Ok(urns_from_refs(refnames).collect())
    }

    fn certifiers_of(&self, peer: &PeerId) -> Result<HashSet<RadUrn>, Error> {
        let git = self.storage.backend();
        let mut refs = git.references_glob(&format!(
            "refs/namespaces/{}/refs/remotes/{}/rad/ids/*",
            &self.urn.id, peer
        ))?;
        let refnames = refs.names();
        Ok(urns_from_refs(refnames).collect())
    }
}

/// Iterator over the 1st degree tracked peers.
///
/// Created by the [`Repo::tracked`] method.
pub struct Tracked {
    remotes: git2::string_array::StringArray,
    range: Range<usize>,
    prefix: String,
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

fn id_branch(namespace: Namespace) -> Reference {
    Reference {
        namespace,
        remote: None,
        category: RefsCategory::Rad,
        name: "id".to_owned(),
    }
}

fn certifiers_glob(namespace: Namespace) -> Reference {
    Reference {
        namespace,
        remote: None,
        category: RefsCategory::Rad,
        name: "ids/*".to_owned(),
    }
}

/// [`Refspec`]s for fetching `rad/refs` in namespace [`Namespace`] from remote
/// peer [`PeerId`], rejecting non-fast-forwards
fn rad_refs_specs<'a>(
    namespace: Namespace,
    remote_peer: &'a PeerId,
    tracked: impl Iterator<Item = &'a PeerId> + 'a,
) -> impl Iterator<Item = Refspec> + 'a {
    tracked.map(move |peer| {
        let local = Reference {
            namespace: namespace.clone(),
            remote: Some((*peer).clone()),
            category: RefsCategory::Rad,
            name: "refs".to_owned(),
        };

        let remote = if peer == remote_peer {
            Reference {
                remote: None,
                ..local.clone()
            }
        } else {
            local.clone()
        };

        Refspec {
            local,
            remote,
            force: false,
        }
    })
}

fn fetch_specs<'a>(
    namespace: Namespace,
    remote_heads: HashMap<&'a str, git2::Oid>,
    tracked_peers: impl Iterator<Item = &'a PeerId> + 'a,
    remote_peer: &'a PeerId,
    rad_refs_of: impl Fn(PeerId) -> Result<Refs, Error>,
    certifiers_of: impl Fn(&PeerId) -> Result<HashSet<RadUrn>, Error>,
) -> Result<impl Iterator<Item = Refspec> + 'a, Error> {
    // FIXME: do this in constant memory
    let mut refspecs = Vec::new();

    for tracked_peer in tracked_peers {
        // Heads
        {
            let their_rad_refs = rad_refs_of(tracked_peer.clone())?;
            for (name, target) in their_rad_refs.heads {
                let targets_match = remote_heads
                    .get(name.as_str())
                    .map(|remote_target| remote_target == &*target)
                    .unwrap_or(false);

                if targets_match {
                    let local = Reference {
                        namespace: namespace.clone(),
                        remote: Some(tracked_peer.clone()),
                        category: RefsCategory::Heads,
                        name,
                    };

                    let remote = if tracked_peer == remote_peer {
                        Reference {
                            remote: None,
                            ..local.clone()
                        }
                    } else {
                        local.clone()
                    };

                    refspecs.push(Refspec {
                        local,
                        remote,
                        force: true,
                    })
                }
            }
        }

        // id and certifiers
        {
            let local = Reference {
                namespace: namespace.clone(),
                remote: Some(tracked_peer.clone()),
                category: RefsCategory::Rad,
                name: "id*".to_owned(),
            };

            let remote = if tracked_peer == remote_peer {
                Reference {
                    remote: None,
                    ..local.clone()
                }
            } else {
                local.clone()
            };

            refspecs.push(Refspec {
                local,
                remote,
                force: false,
            });
        }

        // certifier top-level identities
        {
            let their_certifiers = certifiers_of(&tracked_peer)?;
            for urn in their_certifiers {
                let local = Reference {
                    namespace: urn.id.clone(),
                    remote: Some(tracked_peer.clone()),
                    category: RefsCategory::Rad,
                    name: "id*".to_owned(),
                };

                let remote = if tracked_peer == remote_peer {
                    Reference {
                        remote: None,
                        ..local.clone()
                    }
                } else {
                    local.clone()
                };

                refspecs.push(Refspec {
                    local,
                    remote,
                    force: false,
                });
            }
        }
    }

    Ok(refspecs.into_iter())
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
