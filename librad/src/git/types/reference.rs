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
    fmt::{self, Display},
    marker::PhantomData,
};

use multihash::Multihash;

use crate::{git::ext, hash::Hash, identities, uri::RadUrn};

use super::{
    existential::{SomeNamespace, SomeReference},
    Force,
    Refspec,
    SymbolicRef,
};

/// Type witness for a [`Reference`] that should point to a single reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Single;

/// Type witness for a [`Reference`] that should point to multiple references.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Multiple;

pub type Namespace = Hash;

pub struct Namespace2(ext::Oid);

impl From<ext::Oid> for Namespace2 {
    fn from(oid: ext::Oid) -> Self {
        Self(oid)
    }
}

impl From<git2::Oid> for Namespace2 {
    fn from(oid: git2::Oid) -> Self {
        Self::from(ext::Oid::from(oid))
    }
}

impl From<identities::git::Urn> for Namespace2 {
    fn from(urn: identities::git::Urn) -> Self {
        Self::from(urn.id)
    }
}

impl From<&identities::git::Urn> for Namespace2 {
    fn from(urn: &identities::git::Urn) -> Self {
        Self::from(urn.id)
    }
}

impl Display for Namespace2 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&multibase::encode(
            multibase::Base::Base32Z,
            Multihash::from(&self.0),
        ))
    }
}

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

pub trait ReferenceInfo {
    type Remote;
    type Namespace;
    type Cardinality;
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reference<Namespaced, Remote, Cardinality> {
    /// The remote portion of this reference.
    pub remote: Option<Remote>,
    /// Where this reference falls under, i.e. `rad` or `heads`.
    pub category: RefsCategory,
    /// The path of the reference, e.g. `feature/123`, `dev`.
    pub name: String,

    pub(super) _namespace: Namespaced,
    pub(super) _cardinality: PhantomData<Cardinality>,
}

impl<Namespaced, Remote, Cardinality> ReferenceInfo for Reference<Namespaced, Remote, Cardinality> {
    type Remote = Remote;
    type Namespace = Namespaced;
    type Cardinality = Cardinality;
}

// Polymorphic definitions
impl<Namespaced: Clone, R: Clone, N: Clone> Reference<Namespaced, R, N> {
    /// Set the remote portion of thise reference.
    ///
    /// Note: This is consuming.
    pub fn with_remote(mut self, remote: impl Into<Option<R>>) -> Self {
        self.remote = remote.into();
        self
    }

    /// Set the remote portion of thise reference.
    ///
    /// Note: This is not consuming.
    pub fn set_remote(&self, remote: impl Into<Option<R>>) -> Self {
        Self {
            remote: remote.into(),
            ..self.clone()
        }
    }

    /// Set the namespace of this reference to another one. Note that the
    /// namespace does not have to be of the original namespace's type.
    pub fn with_namespace<Other>(self, namespace: Other) -> Reference<Other, R, N> {
        Reference {
            name: self.name,
            remote: self.remote,
            category: self.category,
            _namespace: namespace,
            _cardinality: self._cardinality,
        }
    }

    /// Existentialise the namespace of the reference.
    pub fn some_namespace(self) -> Reference<SomeNamespace, R, N>
    where
        Namespaced: Into<SomeNamespace>,
    {
        let namespace = self._namespace.clone().into();
        self.with_namespace(namespace)
    }

    /// Set the named portion of this path.
    ///
    /// Note: This is consuming.
    pub fn with_name(mut self, name: &str) -> Self {
        self.name = name.to_owned();
        self
    }

    /// Set the named portion of this path.
    ///
    /// Note: This is not consuming.
    pub fn set_name(&self, name: &str) -> Self {
        Self {
            name: name.to_owned(),
            ..self.clone()
        }
    }

    /// Create the [`Refspec`] using the LHS of this call as the `local`, and
    /// the RHS as the `remote`.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::marker::PhantomData;
    /// use librad::{git::types::*, hash::Hash, keys::SecretKey, peer::PeerId};
    ///
    /// let id = Hash::hash(b"geez");
    /// let peer: PeerId = SecretKey::new().into();
    ///
    /// // Set up a ref to `refs/heads/*`
    /// let flat_heads: FlatRef<String, _> = FlatRef::heads(PhantomData, None);
    ///
    /// // Set up a ref t `refs/namespaces/<geez>/refs/remotes/<peer>/heads/*`
    /// let namespace_heads = NamespacedRef::heads(id, peer.clone());
    ///
    /// // Create a refspec between these two refs
    /// let spec = flat_heads.refspec(namespace_heads, Force::True);
    ///
    /// let expected = format!(
    ///     "+refs/namespaces/hwd1yredksthny1hht3bkhtkxakuzfnjxd8dyk364prfkjxe4xpxsww3try/refs/remotes/{}/heads/*:refs/heads/*",
    ///     peer
    /// );
    ///
    /// assert_eq!(
    ///     &spec.to_string(),
    ///     &expected,
    /// );
    /// ```
    ///
    /// ```
    /// use std::marker::PhantomData;
    /// use librad::{git::types::*, hash::Hash, keys::SecretKey, peer::PeerId};
    ///
    /// let id = Hash::hash(b"geez");
    /// let peer: PeerId = SecretKey::new().into();
    ///
    /// // Set up a ref to `refs/heads/*`
    /// let flat_heads: FlatRef<String, _> = FlatRef::heads(PhantomData, None);
    ///
    /// // Set up a ref t `refs/namespaces/<geez>/refs/remotes/<peer>/heads/banana`
    /// let namespace_head = NamespacedRef::head(id, peer.clone(), "banana");
    ///
    /// // The below would fail to compile because `namespace_head` is a `Single`
    /// // reference while `flat_heads` is `Multiple`.
    /// // let spec = flat_heads.refspec(namespace_head, Force::True);
    /// ```
    pub fn refspec<Other>(self, remote: Other, force: Force) -> Refspec<Other::Remote, R>
    where
        Self: Into<SomeReference<R>>,
        Other:
            Into<SomeReference<<Other as ReferenceInfo>::Remote>> + ReferenceInfo<Cardinality = N>,
    {
        Refspec {
            remote: remote.into(),
            local: self.into(),
            force,
        }
    }
}

// References with a Single cardinality
impl<N, R> Reference<N, R, Single> {
    /// Find this particular reference.
    pub fn find<'a>(&self, repo: &'a git2::Repository) -> Result<git2::Reference<'a>, git2::Error>
    where
        Self: ToString,
    {
        repo.find_reference(&self.to_string())
    }

    /// Create a [`SymbolicRef`] of the `self` parameter where the `source`
    /// parameter will be the newly created reference.
    ///
    /// To create the symbolic reference itself, see [`SymbolicRef::create`].
    pub fn symbolic_ref(&self, source: Self, force: Force) -> SymbolicRef<R>
    where
        R: Clone,
        N: Into<SomeNamespace> + Clone,
    {
        SymbolicRef {
            source: source.clone().with_namespace(source._namespace.into()),
            target: self.clone().with_namespace(self._namespace.clone().into()),
            force,
        }
    }

    /// Build a reference that points to:
    ///     * `refs/namespaces/<namespace>/refs/rad/id`
    pub fn rad_id(namespace: N) -> Self {
        Self {
            remote: None,
            category: RefsCategory::Rad,
            name: "id".to_owned(),
            _namespace: namespace,
            _cardinality: PhantomData,
        }
    }

    /// Build a reference that points to:
    ///     * `refs/namespaces/<namespace>/refs/rad/ids/<id>`
    pub fn rad_certifier(namespace: N, urn: &RadUrn) -> Self {
        Self {
            remote: None,
            category: RefsCategory::Rad,
            name: format!("ids/{}", urn.id),
            _namespace: namespace,
            _cardinality: PhantomData,
        }
    }

    /// Build a reference that points to:
    ///     * `refs/namespaces/<namespace>/refs/rad/signed_refs`
    ///     * `refs/namespaces/<namespace>/refs/remote/<peer_id>/rad/
    ///       signed_refs`
    pub fn rad_signed_refs(namespace: N, remote: impl Into<Option<R>>) -> Self {
        Self {
            remote: remote.into(),
            category: RefsCategory::Rad,
            name: "signed_refs".to_owned(),
            _namespace: namespace,
            _cardinality: PhantomData,
        }
    }

    /// Build a reference that points to:
    ///     * `refs/namespaces/<namespace>/refs/rad/self`
    ///     * `refs/namespaces/<namespace>/refs/remote/<peer_id>/rad/self`
    pub fn rad_self(namespace: N, remote: impl Into<Option<R>>) -> Self {
        Self {
            remote: remote.into(),
            category: RefsCategory::Rad,
            name: "self".to_owned(),
            _namespace: namespace,
            _cardinality: PhantomData,
        }
    }

    /// Build a reference that points to:
    ///     * `refs/namespaces/<namespace>/refs/heads/<name>`
    ///     * `refs/namespaces/<namespace>/refs/remote/<peer_id>/heads/<name>
    pub fn head(namespace: N, remote: impl Into<Option<R>>, name: &str) -> Self {
        Self {
            remote: remote.into(),
            category: RefsCategory::Heads,
            name: name.to_owned(),
            _namespace: namespace,
            _cardinality: PhantomData,
        }
    }
}

// References with a Multiple cardinality
impl<N, R> Reference<N, R, Multiple> {
    /// Get the iterator for these references.
    pub fn references<'a>(
        &self,
        repo: &'a git2::Repository,
    ) -> Result<ext::References<'a>, git2::Error>
    where
        Self: ToString,
    {
        ext::References::from_globs(repo, &[self.to_string()])
    }

    /// Build a reference that points to
    /// `refs/namespaces/<namespace>/refs/rad/ids/*`
    pub fn rad_ids_glob(namespace: N) -> Self {
        Self {
            remote: None,
            category: RefsCategory::Rad,
            name: "ids/*".to_owned(),
            _namespace: namespace,
            _cardinality: PhantomData,
        }
    }

    /// Build a reference that points to
    /// `refs/namespaces/<namespace>/refs/rad/[peer_id]/heads/*`
    pub fn heads(namespace: N, remote: impl Into<Option<R>>) -> Self {
        Self {
            remote: remote.into(),
            category: RefsCategory::Heads,
            name: "*".to_owned(),
            _namespace: namespace,
            _cardinality: PhantomData,
        }
    }
}

impl<'a, N, R> Into<ext::blob::Branch<'a>> for &'a Reference<N, R, Single>
where
    Self: ToString,
{
    fn into(self) -> ext::blob::Branch<'a> {
        ext::blob::Branch::from(self.to_string())
    }
}
