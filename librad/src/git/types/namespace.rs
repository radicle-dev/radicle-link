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
};

use git_ext as ext;
use multihash::Multihash;

use crate::{
    git::sealed,
    hash::Hash,
    identities::urn::{self, Urn},
};

pub trait AsNamespace: Into<ext::RefLike> + sealed::Sealed {
    fn into_namespace(self) -> ext::RefLike {
        self.into()
    }
}

pub type Legacy = Hash;

impl From<Legacy> for ext::RefLike {
    fn from(hash: Legacy) -> Self {
        Self::try_from(hash.to_string()).unwrap()
    }
}

impl From<&Legacy> for ext::RefLike {
    fn from(hash: &Legacy) -> Self {
        Self::try_from(hash.to_string()).unwrap()
    }
}

impl AsNamespace for Legacy {}
impl AsNamespace for &Legacy {}

impl sealed::Sealed for Legacy {}
impl sealed::Sealed for &Legacy {}

#[derive(Debug, Clone, PartialEq)]
pub struct Namespace<R>(Urn<R>);

impl<R> AsNamespace for Namespace<R>
where
    R: urn::HasProtocol,
    for<'a> &'a R: Into<Multihash>,
{
}

impl<'a, R> AsNamespace for &'a Namespace<R>
where
    R: urn::HasProtocol,
    &'a R: Into<Multihash>,
{
}

impl<R> sealed::Sealed for Namespace<R> {}
impl<R> sealed::Sealed for &Namespace<R> {}

impl<R> From<Urn<R>> for Namespace<R> {
    fn from(urn: Urn<R>) -> Self {
        Self(Urn { path: None, ..urn })
    }
}

impl<R: Clone> From<&Urn<R>> for Namespace<R> {
    fn from(urn: &Urn<R>) -> Self {
        Self(Urn {
            path: None,
            id: urn.id.clone(),
        })
    }
}

impl<R> Display for Namespace<R>
where
    R: urn::HasProtocol,
    for<'a> &'a R: Into<Multihash>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(ext::RefLike::from(self).as_str())
    }
}

impl<R> From<Namespace<R>> for ext::RefLike
where
    R: urn::HasProtocol,
    for<'a> &'a R: Into<Multihash>,
{
    fn from(ns: Namespace<R>) -> Self {
        Self::from(ns.0)
    }
}

impl<'a, R> From<&'a Namespace<R>> for ext::RefLike
where
    R: urn::HasProtocol,
    &'a R: Into<Multihash>,
{
    fn from(ns: &'a Namespace<R>) -> Self {
        Self::from(&ns.0)
    }
}

impl<R> From<Namespace<R>> for ext::RefspecPattern
where
    R: urn::HasProtocol,
    for<'a> &'a R: Into<Multihash>,
{
    fn from(ns: Namespace<R>) -> Self {
        ext::RefLike::from(ns).into()
    }
}

impl<'a, R> From<&'a Namespace<R>> for ext::RefspecPattern
where
    R: urn::HasProtocol,
    &'a R: Into<Multihash>,
{
    fn from(ns: &'a Namespace<R>) -> Self {
        ext::RefLike::from(ns).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::identities::urn::tests::FakeId;

    #[test]
    fn is_reflike() {
        let ns = Namespace::from(Urn::new(ext::Oid::from(git2::Oid::zero())));
        assert_eq!(
            "hnrkyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy",
            ext::RefLike::from(ns).as_str()
        )
    }

    #[test]
    fn fake_is_reflike() {
        let ns = Namespace::from(Urn::new(FakeId(42)));
        assert_eq!("hyyryyyyyyyyyyybk", ext::RefLike::from(ns).as_str())
    }

    #[test]
    fn strips_path_from_urn() {
        let ns = Namespace::from(Urn {
            id: FakeId(42),
            path: Some(ext::RefLike::try_from("lolek/bolek").unwrap()),
        });
        assert_eq!("hyyryyyyyyyyyyybk", ext::RefLike::from(ns).as_str())
    }

    #[test]
    fn display_is_reflike_to_str() {
        let ns = Namespace::from(Urn::new(FakeId(69)));
        assert_eq!(&ns.to_string(), ext::RefLike::from(ns).as_str())
    }

    #[test]
    fn reflike_from_ref_from_owned() {
        let ns = Namespace::from(Urn::new(FakeId(666)));
        assert_eq!(ext::RefLike::from(&ns), ext::RefLike::from(ns))
    }
}
