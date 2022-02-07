// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    borrow::{Borrow, Cow},
    convert::TryFrom,
    fmt::{self, Display},
    iter::FromIterator,
    ops::Deref,
};

use thiserror::Error;

use crate::{check, RefStr, RefString};

mod iter;
pub use iter::{Component, Components, Iter};

pub const STAR: &PatternStr = PatternStr::from_str("*");

const CHECK_OPTS: check::Options = check::Options {
    allow_onelevel: true,
    allow_pattern: true,
};

#[repr(transparent)]
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct PatternStr(str);

impl PatternStr {
    #[inline]
    pub fn try_from_str(s: &str) -> Result<&Self, check::Error> {
        TryFrom::try_from(s)
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        self
    }

    pub fn join<R>(&self, other: R) -> PatternString
    where
        R: AsRef<RefStr>,
    {
        self._join(other.as_ref())
    }

    fn _join(&self, other: &RefStr) -> PatternString {
        let mut buf = self.to_owned();
        buf.push(other);
        buf
    }

    #[inline]
    pub fn iter(&self) -> Iter {
        self.0.split('/')
    }

    #[inline]
    pub fn components(&self) -> Components {
        Components::from(self)
    }

    pub(crate) const fn from_str(s: &str) -> &PatternStr {
        unsafe { &*(s as *const str as *const PatternStr) }
    }
}

impl Deref for PatternStr {
    type Target = str;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for PatternStr {
    #[inline]
    fn as_ref(&self) -> &str {
        self
    }
}

impl AsRef<Self> for PatternStr {
    #[inline]
    fn as_ref(&self) -> &Self {
        self
    }
}

impl<'a> TryFrom<&'a str> for &'a PatternStr {
    type Error = check::Error;

    #[inline]
    fn try_from(s: &'a str) -> Result<Self, Self::Error> {
        check::ref_format(CHECK_OPTS, s).map(|()| PatternStr::from_str(s))
    }
}

impl<'a> From<&'a RefStr> for &'a PatternStr {
    #[inline]
    fn from(rs: &'a RefStr) -> Self {
        PatternStr::from_str(rs.as_str())
    }
}

impl<'a> From<&'a PatternStr> for Cow<'a, PatternStr> {
    #[inline]
    fn from(p: &'a PatternStr) -> Cow<'a, PatternStr> {
        Cow::Borrowed(p)
    }
}

impl Display for PatternStr {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self)
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct PatternString(pub(crate) String);

impl PatternString {
    #[inline]
    pub fn as_str(&self) -> &str {
        self.as_ref()
    }

    #[inline]
    pub fn as_pattern_str(&self) -> &PatternStr {
        self.as_ref()
    }

    #[inline]
    pub fn from_components<'a, T>(iter: T) -> Result<Self, DuplicateGlob>
    where
        T: IntoIterator<Item = Component<'a>>,
    {
        iter.into_iter().collect()
    }

    #[inline]
    pub fn and<R>(mut self, other: R) -> Self
    where
        R: AsRef<RefStr>,
    {
        self._push(other.as_ref());
        self
    }

    #[inline]
    pub fn push<R>(&mut self, other: R)
    where
        R: AsRef<RefStr>,
    {
        self._push(other.as_ref())
    }

    fn _push(&mut self, other: &RefStr) {
        self.0.push('/');
        self.0.push_str(other.as_str());
    }

    #[inline]
    pub fn pop(&mut self) -> bool {
        match self.0.rfind('/') {
            None => false,
            Some(idx) => {
                self.0.truncate(idx);
                true
            },
        }
    }
}

impl Deref for PatternString {
    type Target = PatternStr;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.borrow()
    }
}

impl AsRef<PatternStr> for PatternString {
    #[inline]
    fn as_ref(&self) -> &PatternStr {
        self
    }
}

impl AsRef<str> for PatternString {
    #[inline]
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

impl Borrow<PatternStr> for PatternString {
    #[inline]
    fn borrow(&self) -> &PatternStr {
        PatternStr::from_str(self.0.as_str())
    }
}

impl ToOwned for PatternStr {
    type Owned = PatternString;

    #[inline]
    fn to_owned(&self) -> Self::Owned {
        PatternString(self.0.to_owned())
    }
}

impl From<RefString> for PatternString {
    #[inline]
    fn from(rs: RefString) -> Self {
        Self(rs.into())
    }
}

impl<'a> From<&'a PatternString> for Cow<'a, PatternStr> {
    #[inline]
    fn from(p: &'a PatternString) -> Cow<'a, PatternStr> {
        Cow::Borrowed(p.as_ref())
    }
}

impl From<PatternString> for String {
    #[inline]
    fn from(p: PatternString) -> Self {
        p.0
    }
}

impl TryFrom<&str> for PatternString {
    type Error = check::Error;

    #[inline]
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        PatternStr::try_from_str(s).map(ToOwned::to_owned)
    }
}

impl TryFrom<String> for PatternString {
    type Error = check::Error;

    #[inline]
    fn try_from(s: String) -> Result<Self, Self::Error> {
        check::ref_format(CHECK_OPTS, s.as_str()).map(|()| PatternString(s))
    }
}

#[derive(Debug, Error)]
#[error("more than one '*' encountered")]
pub struct DuplicateGlob;

impl<'a> FromIterator<Component<'a>> for Result<PatternString, DuplicateGlob> {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = Component<'a>>,
    {
        use Component::*;

        let mut buf = String::new();
        let mut seen_glob = false;
        for c in iter {
            if let Glob(_) = c {
                if seen_glob {
                    return Err(DuplicateGlob);
                }

                seen_glob = true;
            }

            buf.push_str(c.as_str());
            buf.push('/');
        }
        buf.truncate(buf.len() - 1);

        Ok(PatternString(buf))
    }
}

impl Display for PatternString {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}
