// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    convert::TryFrom,
    fmt::{self, Display},
};

use git_ext as ext;
use thiserror::Error;

use crate::{
    identities,
    peer::{self, PeerId},
};

use super::{sealed, AsNamespace, Force, Namespace};

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
    Notes,
}

impl RefsCategory {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "heads" => Some(Self::Heads),
            "rad" => Some(Self::Rad),
            "tags" => Some(Self::Tags),
            "notes" => Some(Self::Notes),
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
            Self::Notes => f.write_str("notes"),
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
    /// Where this reference falls under, i.e. `heads`, `tags` or`rad`.
    pub category: RefsCategory,
    /// The path of the reference, e.g. `feature/123`, `dev`, `heads/*`.
    pub name: Cardinality,
    /// The namespace of this reference.
    pub namespace: Option<Namespace>,
}

// Polymorphic definitions
impl<N, R, C> Reference<N, R, C>
where
    N: Clone,
    R: Clone,
    C: Clone,
{
    pub fn with_remote(self, remote: impl Into<Option<R>>) -> Self {
        Self {
            remote: remote.into(),
            ..self
        }
    }

    pub fn set_remote(&mut self, remote: impl Into<Option<R>>) {
        self.remote = remote.into();
    }

    pub fn remote(&mut self, remote: impl Into<Option<R>>) -> &mut Self {
        self.set_remote(remote);
        self
    }

    /// Set the namespace of this reference to another one. Note that the
    /// namespace does not have to be of the original namespace's type.
    pub fn with_namespace<NN, Other>(self, namespace: NN) -> Reference<Other, R, C>
    where
        NN: Into<Option<Other>>,
        Other: AsNamespace,
    {
        Reference {
            name: self.name,
            remote: self.remote,
            category: self.category,
            namespace: namespace.into(),
        }
    }

    /// Set the named portion of this path.
    pub fn with_name<S: Into<C>>(self, name: S) -> Self {
        Self {
            name: name.into(),
            ..self
        }
    }

    /// Set the named portion of this path.
    pub fn set_name<S: Into<C>>(&mut self, name: S) {
        self.name = name.into();
    }

    pub fn name<S: Into<C>>(&mut self, name: S) -> &mut Self {
        self.set_name(name);
        self
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
        tracing::debug!(
            "creating direct reference {} -> {} (force: {}, reflog: '{}')",
            self.to_string(),
            target,
            force.as_bool(),
            log_message
        );
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
    pub fn rad_id(namespace: impl Into<Option<N>>) -> Self {
        Self {
            remote: None,
            category: RefsCategory::Rad,
            name: reflike!("id"),
            namespace: namespace.into(),
        }
    }

    /// Build a reference that points to:
    ///     * `refs/namespaces/<namespace>/refs/rad/ids/<id>`
    pub fn rad_delegate(namespace: impl Into<Option<N>>, urn: &Urn) -> Self {
        Self {
            remote: None,
            category: RefsCategory::Rad,
            name: reflike!("ids").join(ext::RefLike::try_from(urn.encode_id()).unwrap()),
            namespace: namespace.into(),
        }
    }

    /// Build a reference that points to:
    ///     * `refs/namespaces/<namespace>/refs/rad/signed_refs`
    ///     * `refs/namespaces/<namespace>/refs/remote/<peer_id>/rad/
    ///       signed_refs`
    pub fn rad_signed_refs(namespace: impl Into<Option<N>>, remote: impl Into<Option<R>>) -> Self {
        Self {
            remote: remote.into(),
            category: RefsCategory::Rad,
            name: reflike!("signed_refs"),
            namespace: namespace.into(),
        }
    }

    /// Build a reference that points to:
    ///     * `refs/namespaces/<namespace>/refs/rad/self`
    ///     * `refs/namespaces/<namespace>/refs/remote/<peer_id>/rad/self`
    pub fn rad_self(namespace: impl Into<Option<N>>, remote: impl Into<Option<R>>) -> Self {
        Self {
            remote: remote.into(),
            category: RefsCategory::Rad,
            name: reflike!("self"),
            namespace: namespace.into(),
        }
    }

    /// Build a reference that points to:
    ///     * `refs/namespaces/<namespace>/refs/heads/<name>`
    ///     * `refs/namespaces/<namespace>/refs/remote/<peer_id>/heads/<name>
    pub fn head(namespace: impl Into<Option<N>>, remote: impl Into<Option<R>>, name: One) -> Self {
        Self {
            remote: remote.into(),
            category: RefsCategory::Heads,
            name,
            namespace: namespace.into(),
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

impl<N, R> From<Reference<N, R, One>> for ext::RefLike
where
    for<'a> &'a N: AsNamespace,
    for<'a> &'a R: AsRemote,
{
    fn from(r: Reference<N, R, One>) -> Self {
        Self::from(&r)
    }
}

impl<'a, N, R> From<&'a Reference<N, R, One>> for ext::RefLike
where
    &'a N: AsNamespace,
    &'a R: AsRemote,
{
    fn from(r: &'a Reference<N, R, One>) -> Self {
        let mut refl = reflike!("refs");

        if let Some(ref namespace) = r.namespace {
            refl = refl
                .join(reflike!("namespaces"))
                .join(namespace)
                .join(reflike!("refs"));
        }
        if let Some(ref remote) = r.remote {
            refl = refl.join(reflike!("remotes")).join(remote);
        }

        refl.join(r.category)
            .join(ext::OneLevel::from(r.name.to_owned()))
    }
}

impl<N, R> From<Reference<N, R, One>> for ext::RefspecPattern
where
    for<'a> &'a N: AsNamespace,
    for<'a> &'a R: AsRemote,
{
    fn from(r: Reference<N, R, One>) -> Self {
        Self::from(&r)
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

// TODO(kim): what is this for?
impl<'a, N, R> Into<ext::blob::Branch<'a>> for &'a Reference<N, R, Single>
where
    Self: ToString,
{
    fn into(self) -> ext::blob::Branch<'a> {
        ext::blob::Branch::from(self.to_string())
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

    /// Build a reference that points to:
    ///     * `refs[/namespaces/<namespace>/refs]/rad/ids/*`
    pub fn rad_ids_glob(namespace: impl Into<Option<N>>) -> Self {
        Self {
            remote: None,
            category: RefsCategory::Rad,
            name: refspec_pattern!("ids/*"),
            namespace: namespace.into(),
        }
    }

    /// Build a reference that points to:
    ///     * `refs[/namespaces/<namespace>/refs][/remotes/<remote>]/heads/*`
    pub fn heads(namespace: impl Into<Option<N>>, remote: impl Into<Option<R>>) -> Self {
        Self {
            remote: remote.into(),
            category: RefsCategory::Heads,
            name: refspec_pattern!("*"),
            namespace: namespace.into(),
        }
    }

    /// Build a reference that points to:
    ///     * `refs[/namespaces/<namespace>]/refs[/remotes/<remote>]/rad/*`
    pub fn rads(namespace: impl Into<Option<N>>, remote: impl Into<Option<R>>) -> Self {
        Self {
            remote: remote.into(),
            category: RefsCategory::Rad,
            name: refspec_pattern!("*"),
            namespace: namespace.into(),
        }
    }

    /// Build a reference that points to:
    ///     * `refs[/namespaces/<namespace>]/refs[/remotes/<remote>]/tags/*`
    pub fn tags(namespace: impl Into<Option<N>>, remote: impl Into<Option<R>>) -> Self {
        Self {
            remote: remote.into(),
            category: RefsCategory::Tags,
            name: refspec_pattern!("*"),
            namespace: namespace.into(),
        }
    }

    /// Build a reference that points to:
    ///     * `refs[/namespaces/<namespace>]/refs[/remotes/<remote>]/notes/*`
    pub fn notes(namespace: impl Into<Option<N>>, remote: impl Into<Option<R>>) -> Self {
        Self {
            remote: remote.into(),
            category: RefsCategory::Notes,
            name: refspec_pattern!("*"),
            namespace: namespace.into(),
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

impl<N, R> From<Reference<N, R, Many>> for ext::RefspecPattern
where
    for<'a> &'a N: AsNamespace,
    for<'a> &'a R: AsRemote,
{
    fn from(r: Reference<N, R, Many>) -> Self {
        Self::from(&r)
    }
}

impl<'a, N, R> From<&'a Reference<N, R, Many>> for ext::RefspecPattern
where
    &'a N: AsNamespace,
    &'a R: AsRemote,
{
    fn from(r: &'a Reference<N, R, Many>) -> Self {
        let mut refl = reflike!("refs");

        if let Some(ref namespace) = r.namespace {
            refl = refl
                .join(reflike!("namespaces"))
                .join(namespace)
                .join(reflike!("refs"));
        }
        if let Some(ref remote) = r.remote {
            refl = refl.join(reflike!("remotes")).join(remote);
        }

        refl.join(r.category).with_pattern_suffix(r.name.to_owned())
    }
}

////////////////////////////////////////////////////////////////////////////////

impl TryFrom<Reference<Namespace<ext::Oid>, PeerId, One>> for Urn {
    type Error = &'static str;

    fn try_from(r: Reference<Namespace<ext::Oid>, PeerId, One>) -> Result<Self, Self::Error> {
        match r.namespace {
            None => Err("missing namespace"),
            Some(ns) => {
                let mut path = reflike!("refs");
                if let Some(remote) = r.remote {
                    path = path.join(reflike!("remotes")).join(remote);
                }
                path = path.join(r.category).join(r.name);

                Ok(Self {
                    path: Some(path),
                    ..Self::from(ns)
                })
            },
        }
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

impl TryFrom<&Urn> for Reference<Namespace<ext::Oid>, PeerId, One> {
    type Error = FromUrnError;

    fn try_from(urn: &Urn) -> Result<Self, Self::Error> {
        let namespace = Namespace::from(urn);
        match &urn.path {
            None => Ok(Self::rad_id(namespace)),

            Some(path) => {
                let path = ext::reference::Qualified::from(path.clone());
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
                            namespace: Some(namespace),
                        })
                    },

                    Some(x) => Ok(Self {
                        remote: None,
                        category: RefsCategory::parse(x).unwrap_or(RefsCategory::Heads),
                        name: iter.map(|x| ext::RefLike::try_from(x).unwrap()).collect(),
                        namespace: Some(namespace),
                    }),

                    None => Err(FromUrnError::Eof),
                }
            },
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
        for<'b> &'b S: Into<ext::RefLike>,
        for<'b> &'b T: Into<ext::RefLike>,
    {
        let source = Into::<ext::RefLike>::into(&self.source);
        let target = Into::<ext::RefLike>::into(&self.target);

        let reflog_msg = &format!("creating symbolic ref {} -> {}", source, target);
        tracing::debug!("{}", reflog_msg);

        repo.find_reference(target.as_str()).and_then(|_| {
            repo.reference_symbolic(
                source.as_str(),
                target.as_str(),
                self.force.as_bool(),
                reflog_msg,
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::keys::SecretKey;

    #[test]
    fn pathless_urn_roundtrip() {
        let urn = Urn::new(git2::Oid::zero().into());
        let as_ref = Reference::try_from(&urn).unwrap();
        assert_eq!(
            urn.with_path(ext::RefLike::from(identities::urn::DEFAULT_PATH.clone())),
            Urn::try_from(as_ref).unwrap()
        )
    }

    #[test]
    fn remotes_path_urn_roundtrip() {
        let peer_id = PeerId::from(SecretKey::new());
        let urn = Urn::new(git2::Oid::zero().into()).with_path(
            reflike!("refs/remotes")
                .join(peer_id)
                .join(reflike!("rad/id")),
        );
        let as_ref = Reference::try_from(&urn).unwrap();
        assert_eq!(urn, Urn::try_from(as_ref).unwrap())
    }

    #[test]
    fn qualified_path_urn_roundtrip() {
        let urn = Urn::new(git2::Oid::zero().into()).with_path(reflike!("refs/rad/id"));
        let as_ref = Reference::try_from(&urn).unwrap();
        assert_eq!(urn, Urn::try_from(as_ref).unwrap())
    }

    #[test]
    fn onelevel_path_urn_roundtrip() {
        let urn = Urn::new(git2::Oid::zero().into()).with_path(reflike!("rad/id"));
        let as_ref = Reference::try_from(&urn).unwrap();
        assert_eq!(
            urn.with_path(reflike!("refs/heads/rad/id")),
            Urn::try_from(as_ref).unwrap()
        )
    }
}
