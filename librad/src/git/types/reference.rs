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
    convert::{TryFrom, TryInto},
    fmt::{self, Display},
};

use git_ext as ext;
use thiserror::Error;

use crate::{
    identities::{self, urn},
    peer::{self, PeerId},
};

use super::{
    namespace::{AsNamespace, Namespace},
    sealed,
    Force,
    Refspec,
    SymbolicRef,
};

use identities::git::Urn;

/// Type witness for a [`Reference`] that should point to a single reference.
pub type One = ext::RefLike;

/// Alias for [`One`].
pub type Single = One;

/// Type witness for a [`Reference`] that should point to multiple references.
pub type Many = ext::RefspecPattern;

/// Alias for [`Many`].
pub type Multiple = Many;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RefsCategory {
    Heads,
    Rad,
    Tags,
}

impl RefsCategory {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "heads" => Some(Self::Heads),
            "rad" => Some(Self::Rad),
            "tags" => Some(Self::Tags),
            _ => None,
        }
    }
}

impl Display for RefsCategory {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Heads => f.write_str("heads"),
            Self::Rad => f.write_str("rad"),
            Self::Tags => f.write_str("tags"),
        }
    }
}

impl From<RefsCategory> for ext::RefLike {
    fn from(cat: RefsCategory) -> Self {
        ext::RefLike::try_from(cat.to_string()).unwrap()
    }
}

/// Ad-hoc trait to prevent the typechecker from recursing.
///
/// Morally, we can convert `Reference<N, R, C>` into `ext::RefLike` for any `R:
/// Into<ext::RefLike>`. However, the typechecker may then attempt to unify `R`
/// with `Reference<_, Reference<_, ...` recursively, leading to
/// non-termination. Hence, we restrict the types which can be used as
/// `Reference::remote` artificially.
pub trait AsRemote: Into<ext::RefLike> + sealed::Sealed {}

impl AsRemote for PeerId {}
impl AsRemote for &PeerId {}

impl AsRemote for ext::RefLike {}
impl AsRemote for &ext::RefLike {}

impl sealed::Sealed for ext::RefLike {}
impl sealed::Sealed for &ext::RefLike {}

#[derive(Debug, Clone, PartialEq)]
pub struct Reference<Namespace, Remote, Cardinality> {
    /// The remote portion of this reference.
    pub remote: Option<Remote>,
    /// Where this reference falls under, i.e. `rad` or `heads`.
    pub category: RefsCategory,
    /// The path of the reference, e.g. `feature/123`, `dev`.
    pub name: Cardinality,

    pub(super) _namespace: Namespace,
}

// Polymorphic definitions
impl<N, R, C> Reference<N, R, C>
where
    N: Clone,
    R: Clone,
    C: Clone,
{
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
    pub fn with_namespace<Other>(self, namespace: Other) -> Reference<Other, R, C> {
        Reference {
            name: self.name,
            remote: self.remote,
            category: self.category,
            _namespace: namespace,
        }
    }

    /// Set the named portion of this path.
    ///
    /// Note: This is consuming.
    pub fn with_name<S: Into<C>>(mut self, name: S) -> Self {
        self.name = name.into();
        self
    }

    /// Set the named portion of this path.
    ///
    /// Note: This is not consuming.
    pub fn set_name<S: Into<C>>(&self, name: S) -> Self {
        Self {
            name: name.into(),
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
    /// use librad::{git_ext as ext, git::types::*, hash::Hash, keys::SecretKey, peer::PeerId};
    ///
    /// let id = Hash::hash(b"geez");
    /// let peer: PeerId = SecretKey::new().into();
    ///
    /// // Set up a ref to `refs/heads/*`
    /// let flat_heads: FlatRef<ext::RefLike, _> = FlatRef::heads(PhantomData, None);
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
    /// use std::{convert::TryFrom, marker::PhantomData};
    /// use librad::{git_ext::RefLike, reflike, git::types::*, hash::Hash, keys::SecretKey, peer::PeerId};
    ///
    /// let id = Hash::hash(b"geez");
    /// let peer: PeerId = SecretKey::new().into();
    ///
    /// // Set up a ref to `refs/heads/*`
    /// let flat_heads: FlatRef<RefLike, _> = FlatRef::heads(PhantomData, None);
    ///
    /// // Set up a ref t `refs/namespaces/<geez>/refs/remotes/<peer>/heads/banana`
    /// let namespace_head = NamespacedRef::head(id, peer.clone(), reflike!("banana"));
    ///
    /// // The below would fail to compile because `namespace_head` is a `Single`
    /// // reference while `flat_heads` is `Multiple`.
    /// // let spec = flat_heads.refspec(namespace_head, Force::True);
    /// ```
    pub fn refspec<RN, RR, RC>(
        self,
        remote: Reference<RN, RR, RC>,
        force: Force,
    ) -> Refspec<Reference<N, R, C>, Reference<RN, RR, RC>> {
        Refspec {
            remote,
            local: self,
            force,
        }
    }
}

// References with a `One` cardinality
impl<N, R> Reference<N, R, One> {
    /// Find this particular reference.
    pub fn find<'a>(&self, repo: &'a git2::Repository) -> Result<git2::Reference<'a>, git2::Error>
    where
        Self: ToString,
    {
        repo.find_reference(&self.to_string())
    }

    pub fn create<'a>(
        &self,
        repo: &'a git2::Repository,
        target: git2::Oid,
        force: super::Force,
        log_message: &str,
    ) -> Result<git2::Reference<'a>, git2::Error>
    where
        Self: ToString,
    {
        repo.reference(&self.to_string(), target, force.as_bool(), log_message)
    }

    /// Create a [`SymbolicRef`] from `source` to `self` as the `target`.
    pub fn symbolic_ref<SN, SR>(
        self,
        source: Reference<SN, SR, Single>,
        force: Force,
    ) -> SymbolicRef<Reference<SN, SR, Single>, Self>
    where
        R: Clone,
        N: Clone,
    {
        SymbolicRef {
            source,
            target: self,
            force,
        }
    }

    /// Build a reference that points to:
    ///     * `refs/namespaces/<namespace>/refs/rad/id`
    pub fn rad_id(namespace: N) -> Self {
        Self {
            remote: None,
            category: RefsCategory::Rad,
            name: reflike!("id"),
            _namespace: namespace,
        }
    }

    /// Build a reference that points to:
    ///     * `refs/namespaces/<namespace>/refs/rad/ids/<id>`
    pub fn rad_delegate(namespace: N, urn: &Urn) -> Self {
        Self {
            remote: None,
            category: RefsCategory::Rad,
            name: format!("ids/{}", urn.id).try_into().unwrap(),
            _namespace: namespace,
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
            name: reflike!("signed_refs"),
            _namespace: namespace,
        }
    }

    /// Build a reference that points to:
    ///     * `refs/namespaces/<namespace>/refs/rad/self`
    ///     * `refs/namespaces/<namespace>/refs/remote/<peer_id>/rad/self`
    pub fn rad_self(namespace: N, remote: impl Into<Option<R>>) -> Self {
        Self {
            remote: remote.into(),
            category: RefsCategory::Rad,
            name: reflike!("self"),
            _namespace: namespace,
        }
    }

    /// Build a reference that points to:
    ///     * `refs/namespaces/<namespace>/refs/heads/<name>`
    ///     * `refs/namespaces/<namespace>/refs/remote/<peer_id>/heads/<name>
    pub fn head(namespace: N, remote: impl Into<Option<R>>, name: One) -> Self {
        Self {
            remote: remote.into(),
            category: RefsCategory::Heads,
            name,
            _namespace: namespace,
        }
    }
}

impl<N, R> Display for Reference<N, R, One>
where
    for<'a> &'a N: AsNamespace,
    for<'a> &'a R: AsRemote,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(Into::<ext::RefLike>::into(self).as_str())
    }
}

impl<'a, N, R> From<&'a Reference<N, R, One>> for ext::RefLike
where
    &'a N: AsNamespace,
    &'a R: AsRemote,
{
    fn from(r: &'a Reference<N, R, One>) -> Self {
        let mut refl = reflike!("refs/namespaces")
            .join(&r._namespace)
            .join(reflike!("refs"));

        if let Some(ref remote) = r.remote {
            refl = refl.join(reflike!("remotes")).join(remote);
        }

        refl.join(r.category)
            .join(ext::OneLevel::from(r.name.to_owned()))
    }
}

impl<'a, N, R> From<&'a Reference<N, R, One>> for ext::RefspecPattern
where
    &'a N: AsNamespace,
    &'a R: AsRemote,
{
    fn from(r: &'a Reference<N, R, One>) -> Self {
        Into::<ext::RefLike>::into(r).into()
    }
}

// References with a `Many` cardinality
impl<N, R> Reference<N, R, Many> {
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
            name: refspec_pattern!("ids/*"),
            _namespace: namespace,
        }
    }

    /// Build a reference that points to
    /// `refs/namespaces/<namespace>/refs/rad/[peer_id]/heads/*`
    pub fn heads(namespace: N, remote: impl Into<Option<R>>) -> Self {
        Self {
            remote: remote.into(),
            category: RefsCategory::Heads,
            name: refspec_pattern!("*"),
            _namespace: namespace,
        }
    }
}

impl<N, R> Display for Reference<N, R, Many>
where
    for<'a> &'a N: AsNamespace,
    for<'a> &'a R: AsRemote,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(Into::<ext::RefspecPattern>::into(self).as_str())
    }
}

impl<'a, N, R> From<&'a Reference<N, R, Many>> for ext::RefspecPattern
where
    &'a N: AsNamespace,
    &'a R: AsRemote,
{
    fn from(r: &'a Reference<N, R, Many>) -> Self {
        let mut refl = reflike!("refs/namespaces")
            .join(&r._namespace)
            .join(reflike!("refs"));

        if let Some(ref remote) = r.remote {
            refl = refl.join(reflike!("remotes")).join(remote);
        }

        refl.join(r.category).with_pattern_suffix(r.name.to_owned())
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

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum FromUrnError {
    #[error("missing {0}")]
    Missing(&'static str),

    #[error("invalid refs category: `{0}`")]
    InvalidCategory(String),

    #[error("early eof")]
    Eof,

    #[error(transparent)]
    PeerId(#[from] peer::conversion::Error),
}

impl TryFrom<&Urn> for Reference<Namespace<ext::Oid>, PeerId, Single> {
    type Error = FromUrnError;

    fn try_from(urn: &Urn) -> Result<Self, Self::Error> {
        let namespace = Namespace::from(urn);
        let path = urn.path.as_ref().unwrap_or(&urn::DEFAULT_PATH);
        let mut iter = path
            .iter()
            .map(|x| x.to_str().expect("RefLike ensures utf8"))
            .skip_while(|x| x == &"refs");

        match iter.next() {
            Some("remotes") => {
                let remote = Some(
                    iter.next()
                        .ok_or(FromUrnError::Missing("remote peer id"))?
                        .parse()?,
                );

                let category = match iter.next() {
                    None => Err(FromUrnError::Missing("category")),
                    Some(x) if x == "heads" => Ok(RefsCategory::Heads),
                    Some(x) if x == "rad" => Ok(RefsCategory::Rad),
                    Some(x) => Err(FromUrnError::InvalidCategory(x.to_owned())),
                }?;

                let name = iter.map(|x| ext::RefLike::try_from(x).unwrap()).collect();

                Ok(Self {
                    remote,
                    category,
                    name,
                    _namespace: namespace,
                })
            },

            Some(x) => Ok(Self {
                remote: None,
                category: RefsCategory::parse(x).unwrap_or(RefsCategory::Heads),
                name: iter.map(|x| ext::RefLike::try_from(x).unwrap()).collect(),
                _namespace: namespace,
            }),

            None => Err(FromUrnError::Eof),
        }
    }
}
