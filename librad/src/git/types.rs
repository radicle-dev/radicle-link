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
    marker::PhantomData,
};

use crate::{
    git::{refs::Refs, sealed},
    hash::Hash,
    peer::PeerId,
    uri::RadUrn,
};

pub mod namespace;
pub mod reference;
pub mod remote;

pub use reference::{Many, Multiple, One, Reference, RefsCategory, Single};

/// A representation of git reference that is either under:
///   * `refs/heads`
///   * `refs/remotes/<origin>`
pub type FlatRef<R, C> = Reference<PhantomData<!>, R, C>;

impl<R: Display, C> Display for FlatRef<R, C> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.remote {
            None => write!(f, "refs/heads/{}", self.name),
            Some(remote) => write!(f, "refs/remotes/{}/{}", remote, self.name),
        }
    }
}

/// A representation of git reference that is under `refs/namespace/<namespace>`
pub type NamespacedRef<N, C> = Reference<N, PeerId, C>;

/// Whether we should force the overwriting of a reference or not.
#[derive(Debug, Clone)]
pub enum Force {
    /// We should overwrite.
    True,
    /// We should not overwrite.
    False,
}

impl Force {
    /// Convert the Force to its `bool` equivalent.
    fn as_bool(&self) -> bool {
        match self {
            Force::True => true,
            Force::False => false,
        }
    }
}

/// The data for creating a symbolic reference in a git repository.
pub struct SymbolicRef<S, T> {
    /// The new symbolic reference.
    pub source: S,
    /// The reference that already exists and we want to create symbolic
    /// reference of.
    pub target: T,
    /// Whether we should overwrite any pre-existing `source`.
    pub force: Force,
}

impl<S, T> SymbolicRef<S, T> {
    /// Create a symbolic reference of `target`, where the `source` is the newly
    /// created reference.
    ///
    /// # Errors
    ///
    ///   * If the `target` does not exist we won't create the symbolic
    ///     reference and we error early.
    ///   * If we could not create the new symbolic reference since the name
    ///     already exists. Note that this will not be the case if `Force::True`
    ///     is passed.
    pub fn create<'a>(&self, repo: &'a git2::Repository) -> Result<git2::Reference<'a>, git2::Error>
    where
        S: Display,
        T: Display,
    {
        let source = self.source.to_string();
        let target = self.target.to_string();

        let reflog_msg = &format!("creating symbolic ref {} -> {}", source, target);
        tracing::info!("{}", reflog_msg);

        repo.find_reference(&target).and_then(|_| {
            repo.reference_symbolic(&source, &target, self.force.as_bool(), reflog_msg)
        })
    }
}

/// Trait object for "existential" [`Refspec`]s.
pub trait AsRefspec: sealed::Sealed {
    fn as_refspec(&self) -> String;
}

pub struct Refspec<Local, Remote> {
    pub local: Local,
    pub remote: Remote,
    pub force: Force,
}

impl<L, R> Refspec<L, R>
where
    L: Display + 'static,
    R: Display + 'static,
{
    pub fn boxed(self) -> Box<dyn AsRefspec> {
        Box::new(self)
    }
}

impl<L, R> AsRefspec for Refspec<L, R>
where
    L: Display,
    R: Display,
{
    fn as_refspec(&self) -> String {
        self.to_string()
    }
}

impl<L, R> sealed::Sealed for Refspec<L, R> {}

impl<L, R> Display for Refspec<L, R>
where
    L: Display,
    R: Display,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.force.as_bool() {
            f.write_str("+")?;
        }
        write!(f, "{}:{}", self.remote, self.local)
    }
}

impl<N> Refspec<Reference<N, PeerId, Single>, Reference<N, PeerId, Single>>
where
    N: Clone,
{
    pub fn rad_signed_refs<'a>(
        namespace: N,
        remote_peer: &'a PeerId,
        tracked: impl Iterator<Item = &'a PeerId> + 'a,
    ) -> impl Iterator<Item = Self> + 'a
    where
        N: 'a,
    {
        tracked.map(move |peer| {
            let local = Reference::rad_signed_refs(namespace.clone(), (*peer).clone());
            let remote = if peer == remote_peer {
                local.set_remote(None)
            } else {
                local.clone()
            };

            local.refspec(remote, Force::False)
        })
    }
}

impl Refspec<Reference<Hash, PeerId, Single>, Reference<Hash, PeerId, Single>> {
    pub fn fetch_heads<'a, E>(
        namespace: Hash,
        remote_heads: HashMap<String, git2::Oid>,
        tracked_peers: impl Iterator<Item = &'a PeerId> + 'a,
        remote_peer: &'a PeerId,
        rad_signed_refs_of: impl Fn(PeerId) -> Result<Refs, E>,
        certifiers_of: impl Fn(&PeerId) -> Result<HashSet<RadUrn>, E>,
    ) -> Result<impl Iterator<Item = Box<dyn AsRefspec>> + 'a, E> {
        // FIXME: do this in constant memory
        let mut refspecs = Vec::new();

        for tracked_peer in tracked_peers {
            // Heads
            //
            // `+refs/namespaces/<namespace>/refs[/remotes/<peer>]/heads/* \
            // :refs/namespaces/<namespace>/refs/remotes/<peer>/refs/heads/*`
            {
                let their_singed_rad_refs = rad_signed_refs_of(tracked_peer.clone())?;
                for (name, target) in their_singed_rad_refs.heads {
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
                                local.set_remote(None)
                            } else {
                                local.clone()
                            };

                            refspecs.push(local.refspec(remote, Force::True).boxed())
                        }
                    }
                }
            }

            // Id
            //
            // `refs/namespaces/<namespace>/refs[/remotes/<peer>]/rad/id \
            // :refs/namespaces/<namespace>/refs/remotes/<peer>/rad/id`
            {
                let local = Reference::rad_id(namespace.clone()).with_remote(tracked_peer.clone());
                let remote = if tracked_peer == remote_peer {
                    local.set_remote(None)
                } else {
                    local.clone()
                };

                refspecs.push(local.refspec(remote, Force::False).boxed());
            }

            // Self
            //
            // `refs/namespaces/<namespace>/refs[/remotes/<peer>]/rad/self \
            // :refs/namespaces/<namespace>/refs/remotes/<peer>/rad/self`
            {
                let local = Reference::rad_self(namespace.clone(), tracked_peer.clone());
                let remote = if tracked_peer == remote_peer {
                    local.set_remote(None)
                } else {
                    local.clone()
                };

                refspecs.push(local.refspec(remote, Force::False).boxed());
            }

            // Certifiers
            //
            // `refs/namespaces/<namespace>/refs[/remotes/<peer>]/rad/ids/* \
            // :refs/namespaces/<namespace>/refs/remotes/<peer>/rad/ids/*`
            {
                let local =
                    Reference::rad_ids_glob(namespace.clone()).with_remote(tracked_peer.clone());
                let remote = if tracked_peer == remote_peer {
                    local.set_remote(None)
                } else {
                    local.clone()
                };

                refspecs.push(local.refspec(remote, Force::False).boxed());
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
                            Reference::rad_id(urn.id.clone()).with_remote(tracked_peer.clone());
                        let remote = if tracked_peer == remote_peer {
                            local.set_remote(None)
                        } else {
                            local.clone()
                        };

                        refspecs.push(local.refspec(remote, Force::False).boxed());
                    }

                    // rad/ids/* of id
                    {
                        let local = Reference::rad_ids_glob(urn.id.clone())
                            .with_remote(tracked_peer.clone());
                        let remote = if tracked_peer == remote_peer {
                            local.set_remote(None)
                        } else {
                            local.clone()
                        };

                        refspecs.push(local.refspec(remote, Force::False).boxed());
                    }
                }
            }
        }

        Ok(refspecs.into_iter())
    }
}
