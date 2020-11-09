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
    convert::TryFrom,
    fmt::{self, Display},
    marker::PhantomData,
};

use git_ext as ext;

use super::sealed;
use crate::peer::PeerId;

pub mod namespace;
pub mod reference;
pub mod remote;

pub use reference::{AsRemote, Many, Multiple, One, Reference, RefsCategory, Single};

/// A representation of git reference that is either under:
///   * `refs/heads`
///   * `refs/remotes/<origin>`
pub type FlatRef<R, C> = Reference<PhantomData<!>, R, C>;

impl<R> Display for FlatRef<R, One>
where
    for<'a> &'a R: AsRemote,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(Into::<ext::RefLike>::into(self).as_str())
    }
}

impl<'a, R> From<&'a FlatRef<R, One>> for ext::RefLike
where
    &'a R: AsRemote,
{
    fn from(r: &'a FlatRef<R, One>) -> Self {
        match r.remote {
            None => ext::Qualified::from(r.name.clone()).into(),
            Some(ref remote) => reflike!("refs/remotes")
                .join(remote)
                .join(ext::OneLevel::from(r.name.clone())),
        }
    }
}

impl<'a, R> From<&'a FlatRef<R, One>> for ext::RefspecPattern
where
    &'a R: AsRemote,
{
    fn from(r: &'a FlatRef<R, One>) -> Self {
        Into::<ext::RefLike>::into(r).into()
    }
}

impl<R> Display for FlatRef<R, Many>
where
    for<'a> &'a R: AsRemote,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(Into::<ext::RefspecPattern>::into(self).as_str())
    }
}

impl<'a, R> From<&'a FlatRef<R, Many>> for ext::RefspecPattern
where
    &'a R: AsRemote,
{
    fn from(r: &'a FlatRef<R, Many>) -> Self {
        let refl = match r.remote {
            None => reflike!("refs/heads"),
            Some(ref remote) => reflike!("refs/remotes").join(remote),
        };

        ext::RefspecPattern::try_from((*refl).join(&r.name)).unwrap()
    }
}

/// A representation of git reference that is under `refs/namespace/<namespace>`
pub type NamespacedRef<N, C> = Reference<N, PeerId, C>;

impl<N, C> NamespacedRef<N, C> {
    pub fn namespace(&self) -> &N {
        &self._namespace
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

impl From<bool> for Force {
    fn from(b: bool) -> Self {
        if b {
            Self::True
        } else {
            Self::False
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

/// Trait for creating "existential" [`Refspec`]s as trait objects.
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
    L: 'static,
    R: 'static,
    for<'a> &'a L: Into<ext::RefspecPattern>,
    for<'a> &'a R: Into<ext::RefspecPattern>,
{
    pub fn boxed(self) -> Box<dyn AsRefspec> {
        Box::new(self)
    }
}

impl<L, R> AsRefspec for Refspec<L, R>
where
    for<'a> &'a L: Into<ext::RefspecPattern>,
    for<'a> &'a R: Into<ext::RefspecPattern>,
{
    fn as_refspec(&self) -> String {
        self.to_string()
    }
}

impl<L, R> sealed::Sealed for Refspec<L, R> {}

impl<L, R> Display for Refspec<L, R>
where
    for<'a> &'a L: Into<ext::RefspecPattern>,
    for<'a> &'a R: Into<ext::RefspecPattern>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.force.as_bool() {
            f.write_str("+")?;
        }

        let remote = Into::<ext::RefspecPattern>::into(&self.remote);
        let local = Into::<ext::RefspecPattern>::into(&self.local);

        write!(f, "{}:{}", remote, local)
    }
}

impl TryFrom<&str> for Refspec<ext::RefspecPattern, ext::RefspecPattern> {
    type Error = ext::reference::name::Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        let force = s.starts_with('+').into();
        let specs = s.trim_start_matches('+');
        let mut iter = specs.split(':');
        let remote = iter
            .next()
            .ok_or(ext::reference::name::Error::RefFormat)
            .and_then(ext::RefspecPattern::try_from)?;
        let local = iter
            .next()
            .ok_or(ext::reference::name::Error::RefFormat)
            .and_then(ext::RefspecPattern::try_from)?;

        Ok(Self {
            remote,
            local,
            force,
        })
    }
}
