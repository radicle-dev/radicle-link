// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    borrow::Cow,
    fmt::{self, Display},
    ops::Deref,
};

use super::{RefStr, RefString};
use crate::lit;

pub type Iter<'a> = std::str::Split<'a, char>;

/// Iterator created by the [`RefStr::iter`] method.
#[must_use = "iterators are lazy and do nothing unless consumed"]
#[derive(Clone)]
pub struct Components<'a> {
    inner: std::str::Split<'a, char>,
}

impl<'a> Iterator for Components<'a> {
    type Item = Component<'a>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner
            .next()
            .map(RefStr::from_str)
            .map(Cow::from)
            .map(Component)
    }
}

impl<'a> From<&'a RefStr> for Components<'a> {
    #[inline]
    fn from(rs: &'a RefStr) -> Self {
        Self {
            inner: rs.as_str().split('/'),
        }
    }
}

/// A path component of a [`RefStr`].
///
/// A [`Component`] is a valid [`RefStr`] which does not contain any '/'
/// separators.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct Component<'a>(Cow<'a, RefStr>);

impl<'a> Component<'a> {
    #[inline]
    pub fn from_refstring(r: RefString) -> Option<Self> {
        if !r.contains('/') {
            Some(Self(Cow::Owned(r)))
        } else {
            None
        }
    }

    #[inline]
    pub fn as_lit<T: lit::Lit>(&self) -> Option<T> {
        T::from_component(self)
    }

    #[inline]
    pub fn into_inner(self) -> Cow<'a, RefStr> {
        self.0
    }
}

impl<'a> Deref for Component<'a> {
    type Target = RefStr;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<RefStr> for Component<'_> {
    #[inline]
    fn as_ref(&self) -> &RefStr {
        self
    }
}

impl<'a> From<&'a RefStr> for Option<Component<'a>> {
    #[inline]
    fn from(r: &'a RefStr) -> Self {
        if !r.contains('/') {
            Some(Component(Cow::from(r)))
        } else {
            None
        }
    }
}

impl<'a> From<Component<'a>> for Cow<'a, RefStr> {
    #[inline]
    fn from(c: Component<'a>) -> Self {
        c.0
    }
}

impl<T: lit::Lit> From<T> for Component<'static> {
    #[inline]
    fn from(_: T) -> Self {
        Component(Cow::from(T::NAME))
    }
}

impl<'a> From<lit::SomeLit<'a>> for Component<'a> {
    #[inline]
    fn from(s: lit::SomeLit<'a>) -> Self {
        use lit::SomeLit::*;

        match s {
            Known(k) => k.into(),
            Any(c) => c,
        }
    }
}

impl Display for Component<'_> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
}

pub mod component {
    use super::Component;
    use crate::name;
    use std::borrow::Cow;

    pub const HEADS: Component = Component(Cow::Borrowed(name::HEADS));
    pub const MAIN: Component = Component(Cow::Borrowed(name::MAIN));
    pub const MASTER: Component = Component(Cow::Borrowed(name::MASTER));
    pub const NAMESPACES: Component = Component(Cow::Borrowed(name::NAMESPACES));
    pub const NOTES: Component = Component(Cow::Borrowed(name::NOTES));
    pub const ORIGIN: Component = Component(Cow::Borrowed(name::ORIGIN));
    pub const REFS: Component = Component(Cow::Borrowed(name::REFS));
    pub const REMOTES: Component = Component(Cow::Borrowed(name::REMOTES));
    pub const TAGS: Component = Component(Cow::Borrowed(name::TAGS));

    #[cfg(feature = "link-literals")]
    mod link {
        use super::*;

        pub const RAD: Component = Component(Cow::Borrowed(name::RAD));
        pub const ID: Component = Component(Cow::Borrowed(name::ID));
        pub const IDS: Component = Component(Cow::Borrowed(name::IDS));
        pub const SELF: Component = Component(Cow::Borrowed(name::SELF));
        pub const SIGNED_REFS: Component = Component(Cow::Borrowed(name::SIGNED_REFS));
        pub const COBS: Component = Component(Cow::Borrowed(name::COBS));
    }
    #[cfg(feature = "link-literals")]
    pub use link::*;
}
