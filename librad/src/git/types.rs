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
    collections::{HashMap, HashSet},
    fmt::{self, Display},
};

use crate::{
    git::{ext, refs::Refs},
    hash::Hash,
    peer::PeerId,
    uri::RadUrn,
};

pub type Namespace = Hash;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RefsCategory {
    Heads,
    Rad,
}

impl Display for RefsCategory {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Heads => f.write_str("heads"),
            Self::Rad => f.write_str("rad"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Reference {
    pub namespace: Namespace,
    pub remote: Option<PeerId>,
    pub category: RefsCategory,
    pub name: String, // TODO: apply validation like `uri::Path`
}

impl Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "refs/namespaces/{}/refs/", self.namespace)?;

        match &self.remote {
            None => write!(f, "{}/{}", self.category, self.name),
            Some(remote) => write!(f, "remotes/{}/{}/{}", remote, self.category, self.name),
        }
    }
}

impl Reference {
    pub fn find<'a>(&self, repo: &'a git2::Repository) -> Result<git2::Reference<'a>, git2::Error> {
        repo.find_reference(&self.to_string())
    }

    pub fn rad_id(namespace: Namespace) -> Self {
        Self {
            namespace,
            remote: None,
            category: RefsCategory::Rad,
            name: "id".to_owned(),
        }
    }

    pub fn rad_ids_glob(namespace: Namespace) -> Self {
        Self {
            namespace,
            remote: None,
            category: RefsCategory::Rad,
            name: "ids/*".to_owned(),
        }
    }

    pub fn rad_certifier(namespace: Namespace, urn: &RadUrn) -> Self {
        Self {
            namespace,
            remote: None,
            category: RefsCategory::Rad,
            name: format!("ids/{}", urn.id),
        }
    }

    pub fn rad_refs(namespace: Namespace, remote: impl Into<Option<PeerId>>) -> Self {
        Self {
            namespace,
            remote: remote.into(),
            category: RefsCategory::Rad,
            name: "refs".to_owned(),
        }
    }

    pub fn rad_self(namespace: Namespace, remote: impl Into<Option<PeerId>>) -> Self {
        Self {
            namespace,
            remote: remote.into(),
            category: RefsCategory::Rad,
            name: "self".to_owned(),
        }
    }

    pub fn head(namespace: Namespace, remote: impl Into<Option<PeerId>>, name: &str) -> Self {
        Self {
            namespace,
            remote: remote.into(),
            category: RefsCategory::Heads,
            name: name.to_owned(),
        }
    }

    pub fn set_remote(mut self, remote: impl Into<Option<PeerId>>) -> Self {
        self.remote = remote.into();
        self
    }

    pub fn with_remote(&self, remote: impl Into<Option<PeerId>>) -> Self {
        Self {
            remote: remote.into(),
            ..self.clone()
        }
    }

    pub fn set_name(mut self, name: &str) -> Self {
        self.name = name.to_owned();
        self
    }

    pub fn with_name(&self, name: &str) -> Self {
        Self {
            name: name.to_owned(),
            ..self.clone()
        }
    }
}

impl<'a> Into<ext::blob::Branch<'a>> for &'a Reference {
    fn into(self) -> ext::blob::Branch<'a> {
        ext::blob::Branch::from(self.to_string())
    }
}

pub struct SymbolicReference<'a> {
    repo: &'a git2::Repository,
}

pub struct SymbolicReferenceSource<'a> {
    repo: &'a git2::Repository,
    source: String,
}

pub struct SymbolicReferenceTarget<'a> {
    repo: &'a git2::Repository,
    source: String,
    target: String,
}

pub struct SymbolicReferenceForce<'a> {
    repo: &'a git2::Repository,
    source: String,
    target: String,
    force_value: bool,
}

/// Builder to create a symbolic reference on the given git repo
///
/// Look at the subsequent `SymbolicReferenceSource`, `SymbolicReferenceTarget`
/// and `SymbolicReferenceForce` types to see the full API.
impl<'a> SymbolicReference<'a> {
    /// Create the builder
    pub fn new(repo: &'a git2::Repository) -> Self {
        Self { repo }
    }

    /// Specify the symbolic reference source
    pub fn source(self, source: &'a Reference) -> SymbolicReferenceSource {
        SymbolicReferenceSource {
            repo: self.repo,
            source: source.to_string(),
        }
    }
}

/// Intermediate symbolic reference builder
impl<'a> SymbolicReferenceSource<'a> {
    /// Specify the symbolic reference target
    /// (checks that the ref exists and fails otherwise)
    pub fn target(self, target: &'a Reference) -> Result<SymbolicReferenceTarget, git2::Error> {
        target.find(self.repo)?;
        Ok(SymbolicReferenceTarget {
            repo: self.repo,
            source: self.source,
            target: target.to_string(),
        })
    }

    /// Specify the symbolic reference target using a `git2::Reference` that we
    /// trust to exist (fails if its `name` is not a valid string)
    pub fn target_ref(
        self,
        target: &'a git2::Reference,
    ) -> Result<SymbolicReferenceTarget<'a>, git2::Error> {
        Ok(SymbolicReferenceTarget {
            repo: self.repo,
            source: self.source,
            target: target
                .name()
                .ok_or_else(|| git2::Error::from_str("Invalid target reference name"))?
                .to_owned(),
        })
    }
}

/// Intermediate symbolic reference builder
impl<'a> SymbolicReferenceTarget<'a> {
    /// Sets the `force` argument to use when creating the reference
    pub fn force(self, force: bool) -> SymbolicReferenceForce<'a> {
        SymbolicReferenceForce {
            repo: self.repo,
            source: self.source,
            target: self.target,
            force_value: force,
        }
    }
}

/// Final symbolic reference builder
impl<'a> SymbolicReferenceForce<'a> {
    /// Creates the symbolic reference
    pub fn create(self) -> Result<git2::Reference<'a>, git2::Error> {
        self.repo.reference_symbolic(
            &self.source,
            &self.target,
            self.force_value,
            &format!("creating symref {} -> {}", self.source, self.target),
        )
    }
}

#[derive(Clone)]
pub struct Refspec {
    pub remote: Reference,
    pub local: Reference,
    pub force: bool,
}

impl Display for Refspec {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.force {
            f.write_str("+")?;
        }
        write!(f, "{}:{}", self.remote, self.local)
    }
}

impl Refspec {
    /// [`Refspec`]s for fetching `rad/refs` in namespace [`Namespace`] from
    /// remote peer [`PeerId`], rejecting non-fast-forwards
    pub fn rad_refs<'a>(
        namespace: Namespace,
        remote_peer: &'a PeerId,
        tracked: impl Iterator<Item = &'a PeerId> + 'a,
    ) -> impl Iterator<Item = Self> + 'a {
        tracked.map(move |peer| {
            let local = Reference::rad_refs(namespace.clone(), (*peer).clone());
            let remote = if peer == remote_peer {
                local.with_remote(None)
            } else {
                local.clone()
            };

            Self {
                local,
                remote,
                force: false,
            }
        })
    }

    pub fn fetch_heads<'a, E>(
        namespace: Namespace,
        remote_heads: HashMap<String, git2::Oid>,
        tracked_peers: impl Iterator<Item = &'a PeerId> + 'a,
        remote_peer: &'a PeerId,
        rad_refs_of: impl Fn(PeerId) -> Result<Refs, E>,
        certifiers_of: impl Fn(&PeerId) -> Result<HashSet<RadUrn>, E>,
    ) -> Result<impl Iterator<Item = Self> + 'a, E> {
        // FIXME: do this in constant memory
        let mut refspecs = Vec::new();

        for tracked_peer in tracked_peers {
            // Heads
            //
            // `+refs/namespaces/<namespace>/refs[/remotes/<peer>]/heads/* \
            // :refs/namespaces/<namespace>/refs/remotes/<peer>/refs/heads/*`
            {
                let their_rad_refs = rad_refs_of(tracked_peer.clone())?;
                for (name, target) in their_rad_refs.heads {
                    let name_namespaced = format!("refs/namespaces/{}/{}", namespace, name);
                    if let Some(name) = name.strip_prefix("refs/heads/") {
                        let targets_match = remote_heads
                            .get(name_namespaced.as_str())
                            .map(|remote_target| remote_target == &*target)
                            .unwrap_or(false);

                        if targets_match {
                            let local =
                                Reference::head(namespace.clone(), tracked_peer.clone(), &name);
                            let remote = if tracked_peer == remote_peer {
                                local.with_remote(None)
                            } else {
                                local.clone()
                            };

                            refspecs.push(Self {
                                local,
                                remote,
                                force: true,
                            })
                        }
                    }
                }
            }

            // Id
            //
            // `refs/namespaces/<namespace>/refs[/remotes/<peer>]/rad/id \
            // :refs/namespaces/<namespace>/refs/remotes/<peer>/rad/id`
            {
                let local = Reference::rad_id(namespace.clone()).set_remote(tracked_peer.clone());
                let remote = if tracked_peer == remote_peer {
                    local.with_remote(None)
                } else {
                    local.clone()
                };

                refspecs.push(Self {
                    local,
                    remote,
                    force: false,
                });
            }

            // Self
            //
            // `refs/namespaces/<namespace>/refs[/remotes/<peer>]/rad/self \
            // :refs/namespaces/<namespace>/refs/remotes/<peer>/rad/self`
            {
                let local = Reference::rad_self(namespace.clone(), tracked_peer.clone());
                let remote = if tracked_peer == remote_peer {
                    local.with_remote(None)
                } else {
                    local.clone()
                };

                refspecs.push(Self {
                    local,
                    remote,
                    force: false,
                })
            }

            // Certifiers
            //
            // `refs/namespaces/<namespace>/refs[/remotes/<peer>]/rad/ids/* \
            // :refs/namespaces/<namespace>/refs/remotes/<peer>/rad/ids/*`
            {
                let local =
                    Reference::rad_ids_glob(namespace.clone()).set_remote(tracked_peer.clone());
                let remote = if tracked_peer == remote_peer {
                    local.with_remote(None)
                } else {
                    local.clone()
                };

                refspecs.push(Self {
                    local,
                    remote,
                    force: false,
                });
            }

            // Certifier top-level identities
            //
            // `refs/namespaces/<certifier>/refs[/remotes/<peer>]/rad/id \
            // :refs/namespaces/<certifier>/refs/remotes/<peer>/rad/id`
            //
            // and
            //
            // `refs/namespaces/<certifier>/refs[/remotes/<peer>]/rad/ids/* \
            // :refs/namespaces/<certifier>/refs/remotes/<peer>/rad/ids/*`
            {
                let their_certifiers = certifiers_of(&tracked_peer)?;
                for urn in their_certifiers {
                    // id
                    {
                        let local =
                            Reference::rad_id(urn.id.clone()).set_remote(tracked_peer.clone());
                        let remote = if tracked_peer == remote_peer {
                            local.with_remote(None)
                        } else {
                            local.clone()
                        };

                        refspecs.push(Self {
                            local,
                            remote,
                            force: false,
                        });
                    }

                    // rad/ids/* of id
                    {
                        let local = Reference::rad_ids_glob(urn.id.clone())
                            .set_remote(tracked_peer.clone());
                        let remote = if tracked_peer == remote_peer {
                            local.with_remote(None)
                        } else {
                            local.clone()
                        };

                        refspecs.push(Self {
                            local,
                            remote,
                            force: false,
                        });
                    }
                }
            }
        }

        Ok(refspecs.into_iter())
    }
}
