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

/// Type witness for a [`Reference`] that should point to a single reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Single;

/// Type witness for a [`Reference`] that should point to multiple references.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Multiple;

#[derive(Debug, Clone, PartialEq)]
pub enum SomeReference {
    Single(Reference<Single>),
    Multiple(Reference<Multiple>),
    BranchMultiple(BranchRef<Multiple>),
    BranchSingle(BranchRef<Single>),
}

impl Display for SomeReference {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Single(reference) => write!(f, "{}", reference),
            Self::Multiple(reference) => write!(f, "{}", reference),
            Self::BranchMultiple(reference) => write!(f, "{}", reference),
            Self::BranchSingle(reference) => write!(f, "{}", reference),
        }
    }
}

/// A representation of git branch that is either under:
///   * `refs/heads`
///   * `refs/remotes/<origin>`
#[derive(Debug, Clone, PartialEq)]
pub struct BranchRef<N> {
    pub remote: Option<String>,
    pub name: String,
    marker: PhantomData<N>,
}

impl<N> Display for BranchRef<N> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.remote {
            None => write!(f, "refs/heads/"),
            Some(remote) => write!(f, "refs/remotes/{}/", remote),
        }?;

        write!(f, "{}", self.name)
    }
}

impl<N> BranchRef<N> {
    /// Set the git reference's remote.
    ///
    /// # Examples
    ///
    /// ```
    /// use librad::git::types::BranchRef;
    ///
    /// let heads = BranchRef::heads().set_remote("origin".to_string());
    /// assert_eq!(&heads.to_string(), "refs/remotes/origin/*");
    /// ```
    pub fn set_remote(mut self, remote: impl Into<Option<String>>) -> Self {
        self.remote = remote.into();
        self
    }

    /// Set the git reference's remote, while not consuming the original
    /// reference.
    ///
    /// # Examples
    ///
    /// ```
    /// use librad::git::types::BranchRef;
    ///
    /// let heads = BranchRef::heads();
    /// let origin_heads = heads.with_remote("origin".to_string());
    ///
    /// assert_eq!(&origin_heads.to_string(), "refs/remotes/origin/*");
    /// assert_eq!(&heads.to_string(), "refs/heads/*");
    /// ```
    pub fn with_remote(&self, remote: impl Into<Option<String>>) -> Self
    where
        N: Clone,
    {
        Self {
            remote: remote.into(),
            ..self.clone()
        }
    }

    pub fn set_name(mut self, name: &str) -> Self {
        self.name = name.to_owned();
        self
    }

    pub fn with_name(&self, name: &str) -> Self
    where
        N: Clone,
    {
        Self {
            name: name.to_owned(),
            ..self.clone()
        }
    }
}

impl BranchRef<Multiple> {
    /// A git reference that corresponds to a wildcard match in `heads`
    ///
    /// # Examples
    ///
    /// ```
    /// use librad::git::types::BranchRef;
    ///
    /// let heads = BranchRef::heads();
    /// assert_eq!(&heads.to_string(), "refs/heads/*")
    /// ```
    pub fn heads() -> Self {
        Self {
            remote: None,
            name: "*".to_string(),
            marker: PhantomData,
        }
    }

    /// Create a `Refspec` where the `BranchRef` is the RHS and the `Reference`
    /// is the LHS.
    ///
    /// This allows us to create a *fetch spec* that links a working copy to the
    /// monorepo.
    ///
    /// # Examples
    ///
    /// ```
    /// use librad::{
    ///     hash::Hash,
    ///     git::types::{BranchRef, Reference, Force},
    /// };
    ///
    /// let namespace = Hash::hash(b"heelflip");
    /// let spec = BranchRef::heads().local_spec(Reference::heads(namespace, None), Force::True);
    /// assert_eq!(
    ///     &spec.to_string(),
    ///     "+refs/namespaces/hwd1yref3ituqkp7ndjnzjzbb8b1taxq54e14y5nzswsg9kuwtb6nbccr3a/refs/heads/*:refs/heads/*"
    /// );
    /// ```
    pub fn local_spec(self, remote: Reference<Multiple>, force: Force) -> Refspec {
        Refspec {
            local: SomeReference::BranchMultiple(self),
            remote: SomeReference::Multiple(remote),
            force,
        }
    }

    /// Create a `Refspec` where the `BranchRef` is the RHS and the `Reference`
    /// is the LHS.
    ///
    /// This allows us to create a *push spec* that links a working copy to the
    /// monorepo.
    ///
    /// # Examples
    ///
    /// ```
    /// use librad::{
    ///     hash::Hash,
    ///     git::types::{BranchRef, Reference, Force},
    /// };
    ///
    /// let namespace = Hash::hash(b"heelflip");
    /// let spec = BranchRef::heads().remote_spec(Reference::heads(namespace, None), Force::True);
    /// assert_eq!(
    ///     &spec.to_string(),
    ///     "+refs/heads/*:refs/namespaces/hwd1yref3ituqkp7ndjnzjzbb8b1taxq54e14y5nzswsg9kuwtb6nbccr3a/refs/heads/*"
    /// );
    /// ```
    pub fn remote_spec(self, local: Reference<Multiple>, force: Force) -> Refspec {
        Refspec {
            local: SomeReference::Multiple(local),
            remote: SomeReference::BranchMultiple(self),
            force,
        }
    }
}

impl BranchRef<Single> {
    /// A git reference that corresponds to a single path in `heads`
    ///
    /// # Examples
    ///
    /// ```
    /// use librad::git::types::BranchRef;
    ///
    /// let heads = BranchRef::head("kickflip");
    /// assert_eq!(&heads.to_string(), "refs/heads/kickflip")
    /// ```
    pub fn head(name: &str) -> Self {
        Self {
            remote: None,
            name: name.to_string(),
            marker: PhantomData,
        }
    }

    pub fn local_spec(self, remote: Reference<Single>, force: Force) -> Refspec {
        Refspec {
            local: SomeReference::BranchSingle(self),
            remote: SomeReference::Single(remote),
            force,
        }
    }

    pub fn remote_spec(self, local: Reference<Single>, force: Force) -> Refspec {
        Refspec {
            local: SomeReference::Single(local),
            remote: SomeReference::BranchSingle(self),
            force,
        }
    }
}

/// A structure for building a git reference.
#[derive(Clone, Debug, PartialEq)]
pub struct Reference<N> {
    /// The namespace the reference lives under.
    pub namespace: Namespace,
    /// The remote peer the reference might live under.
    pub remote: Option<PeerId>,
    /// The category of the reference.
    pub category: RefsCategory,
    /// The suffix path of the reference, e.g. `self`, `id`, `banana/pineapple`.
    pub name: String, // TODO: apply validation like `uri::Path`

    /// Type witness for the cardinality this reference could point to, i.e. if
    /// we are pointing to exactly one, zero or more, or both.
    marker: PhantomData<N>,
}

impl<N> Display for Reference<N> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "refs/namespaces/{}/refs/", self.namespace)?;

        match &self.remote {
            None => write!(f, "{}/{}", self.category, self.name),
            Some(remote) => write!(f, "remotes/{}/{}/{}", remote, self.category, self.name),
        }
    }
}

impl<N: Clone> Reference<N> {
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

impl Reference<Multiple> {
    pub fn references<'a>(
        &self,
        repo: &'a git2::Repository,
    ) -> Result<ext::References<'a>, git2::Error> {
        ext::References::from_globs(repo, &[self.to_string()])
    }

    /// Create the [`Refspec`] using the LHS of this call as the `local`, and
    /// the RHS as the `remote`.
    pub fn refspec(self, remote: Self, force: Force) -> Refspec {
        Refspec {
            local: SomeReference::Multiple(self),
            remote: SomeReference::Multiple(remote),
            force,
        }
    }

    /// Build a reference that points to
    /// `refs/namespaces/<namespace>/refs/rad/ids/*`
    pub fn rad_ids_glob(namespace: Namespace) -> Self {
        Self {
            namespace,
            remote: None,
            category: RefsCategory::Rad,
            name: "ids/*".to_owned(),
            marker: PhantomData,
        }
    }

    /// Build a reference that points to
    /// `refs/namespaces/<namespace>/refs/rad/[peer_id]/heads/*`
    pub fn heads(namespace: Namespace, remote: impl Into<Option<PeerId>>) -> Self {
        Self {
            namespace,
            remote: remote.into(),
            category: RefsCategory::Heads,
            name: "*".to_owned(),
            marker: PhantomData,
        }
    }
}

impl Reference<Single> {
    pub fn find<'a>(&self, repo: &'a git2::Repository) -> Result<git2::Reference<'a>, git2::Error> {
        repo.find_reference(&self.to_string())
    }

    /// Create a [`SymbolicRef`] of the `self` parameter where the `source`
    /// parameter will be the newly created reference.
    ///
    /// To create the symbolic reference itself, see [`SymbolicRef::create`].
    pub fn symbolic_ref(&self, source: Self, force: Force) -> SymbolicRef {
        SymbolicRef {
            source,
            target: self.clone(),
            force,
        }
    }

    /// Create the [`Refspec`] using the LHS of this call as the `local`, and
    /// the RHS as the `remote`.
    pub fn refspec(self, remote: Self, force: Force) -> Refspec {
        Refspec {
            local: SomeReference::Single(self),
            remote: SomeReference::Single(remote),
            force,
        }
    }

    /// Build a reference that points to:
    ///     * `refs/namespaces/<namespace>/refs/rad/id`
    pub fn rad_id(namespace: Namespace) -> Self {
        Self {
            namespace,
            remote: None,
            category: RefsCategory::Rad,
            name: "id".to_owned(),
            marker: PhantomData,
        }
    }

    /// Build a reference that points to:
    ///     * `refs/namespaces/<namespace>/refs/rad/ids/<id>`
    pub fn rad_certifier(namespace: Namespace, urn: &RadUrn) -> Self {
        Self {
            namespace,
            remote: None,
            category: RefsCategory::Rad,
            name: format!("ids/{}", urn.id),
            marker: PhantomData,
        }
    }

    /// Build a reference that points to:
    ///     * `refs/namespaces/<namespace>/refs/rad/signed_refs`
    ///     * `refs/namespaces/<namespace>/refs/remote/<peer_id>/rad/
    ///       signed_refs`
    pub fn rad_signed_refs(namespace: Namespace, remote: impl Into<Option<PeerId>>) -> Self {
        Self {
            namespace,
            remote: remote.into(),
            category: RefsCategory::Rad,
            name: "signed_refs".to_owned(),
            marker: PhantomData,
        }
    }

    /// Build a reference that points to:
    ///     * `refs/namespaces/<namespace>/refs/rad/self`
    ///     * `refs/namespaces/<namespace>/refs/remote/<peer_id>/rad/self`
    pub fn rad_self(namespace: Namespace, remote: impl Into<Option<PeerId>>) -> Self {
        Self {
            namespace,
            remote: remote.into(),
            category: RefsCategory::Rad,
            name: "self".to_owned(),
            marker: PhantomData,
        }
    }

    /// Build a reference that points to:
    ///     * `refs/namespaces/<namespace>/refs/heads/<name>`
    ///     * `refs/namespaces/<namespace>/refs/remote/<peer_id>/heads/<name>
    pub fn head(namespace: Namespace, remote: impl Into<Option<PeerId>>, name: &str) -> Self {
        Self {
            namespace,
            remote: remote.into(),
            category: RefsCategory::Heads,
            name: name.to_owned(),
            marker: PhantomData,
        }
    }
}

impl<'a> Into<ext::blob::Branch<'a>> for &'a Reference<Single> {
    fn into(self) -> ext::blob::Branch<'a> {
        ext::blob::Branch::from(self.to_string())
    }
}

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
pub struct SymbolicRef {
    /// The new symbolic reference.
    pub source: Reference<Single>,
    /// The reference that already exists and we want to create symbolic
    /// reference of.
    pub target: Reference<Single>,
    /// Whether we should overwrite any pre-existing `source`.
    pub force: Force,
}

impl SymbolicRef {
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
    pub fn create<'a>(
        &self,
        repo: &'a git2::Repository,
    ) -> Result<git2::Reference<'a>, git2::Error> {
        let source = self.source.to_string();
        let target = self.target.to_string();

        let sym_log_msg = &format!("creating symbolic ref {} -> {}", source, target);
        tracing::info!("{}", sym_log_msg);

        repo.find_reference(&target).and_then(|_| {
            repo.reference_symbolic(&source, &target, self.force.as_bool(), sym_log_msg)
        })
    }
}

#[derive(Clone)]
pub struct Refspec {
    pub(crate) remote: SomeReference,
    pub(crate) local: SomeReference,
    pub force: Force,
}

impl Display for Refspec {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.force.as_bool() {
            f.write_str("+")?;
        }
        write!(f, "{}:{}", self.remote, self.local)
    }
}

impl Refspec {
    /// [`Refspec`]s for fetching `rad/refs` in namespace [`Namespace`] from
    /// remote peer [`PeerId`], rejecting non-fast-forwards
    pub fn rad_signed_refs<'a>(
        namespace: Namespace,
        remote_peer: &'a PeerId,
        tracked: impl Iterator<Item = &'a PeerId> + 'a,
    ) -> impl Iterator<Item = Self> + 'a {
        tracked.map(move |peer| {
            let local = Reference::rad_signed_refs(namespace.clone(), (*peer).clone());
            let remote = if peer == remote_peer {
                local.with_remote(None)
            } else {
                local.clone()
            };

            local.refspec(remote, Force::False)
        })
    }

    pub fn fetch_heads<'a, E>(
        namespace: Namespace,
        remote_heads: HashMap<String, git2::Oid>,
        tracked_peers: impl Iterator<Item = &'a PeerId> + 'a,
        remote_peer: &'a PeerId,
        rad_signed_refs_of: impl Fn(PeerId) -> Result<Refs, E>,
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
                                local.with_remote(None)
                            } else {
                                local.clone()
                            };

                            refspecs.push(local.refspec(remote, Force::True))
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

                refspecs.push(local.refspec(remote, Force::False));
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

                refspecs.push(local.refspec(remote, Force::False));
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

                refspecs.push(local.refspec(remote, Force::False));
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

                        refspecs.push(local.refspec(remote, Force::False));
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

                        refspecs.push(local.refspec(remote, Force::False));
                    }
                }
            }
        }

        Ok(refspecs.into_iter())
    }
}
