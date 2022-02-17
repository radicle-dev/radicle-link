// Copyright © 2022 The Radicle Link Contributors
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

use crate::{
    check,
    refspec::{PatternStr, PatternString},
    Namespaced,
    Qualified,
};

mod iter;
pub use iter::{component, Component, Components, Iter};

#[cfg(feature = "percent-encoding")]
pub use percent_encoding::PercentEncode;

pub const HEADS: &RefStr = RefStr::from_str(str::HEADS);
pub const MAIN: &RefStr = RefStr::from_str(str::MAIN);
pub const MASTER: &RefStr = RefStr::from_str(str::MASTER);
pub const NAMESPACES: &RefStr = RefStr::from_str(str::NAMESPACES);
pub const NOTES: &RefStr = RefStr::from_str(str::NOTES);
pub const ORIGIN: &RefStr = RefStr::from_str(str::ORIGIN);
pub const REFS: &RefStr = RefStr::from_str(str::REFS);
pub const REMOTES: &RefStr = RefStr::from_str(str::REMOTES);
pub const TAGS: &RefStr = RefStr::from_str(str::TAGS);

pub const REFS_HEADS_MAIN: &RefStr = RefStr::from_str(str::REFS_HEADS_MAIN);
pub const REFS_HEADS_MASTER: &RefStr = RefStr::from_str(str::REFS_HEADS_MASTER);

pub mod str {
    pub const HEADS: &str = "heads";
    pub const MAIN: &str = "main";
    pub const MASTER: &str = "master";
    pub const NAMESPACES: &str = "namespaces";
    pub const NOTES: &str = "notes";
    pub const ORIGIN: &str = "origin";
    pub const REFS: &str = "refs";
    pub const REMOTES: &str = "remotes";
    pub const TAGS: &str = "tags";

    pub const REFS_HEADS_MAIN: &str = "refs/heads/main";
    pub const REFS_HEADS_MASTER: &str = "refs/heads/master";

    #[cfg(feature = "link-literals")]
    mod link {
        pub const RAD: &str = "rad";
        pub const ID: &str = "id";
        pub const IDS: &str = "ids";
        pub const SELF: &str = "self";
        pub const SIGNED_REFS: &str = "signed_refs";
        pub const COBS: &str = "cobs";

        pub const REFS_RAD_ID: &str = "refs/rad/id";
        pub const REFS_RAD_SELF: &str = "refs/rad/self";
        pub const REFS_RAD_SIGNED_REFS: &str = "refs/rad/signed_refs";
    }
    #[cfg(feature = "link-literals")]
    pub use link::*;
}

pub mod bytes {
    use super::str;

    pub const HEADS: &[u8] = str::HEADS.as_bytes();
    pub const MAIN: &[u8] = str::MAIN.as_bytes();
    pub const MASTER: &[u8] = str::MASTER.as_bytes();
    pub const NAMESPACES: &[u8] = str::NAMESPACES.as_bytes();
    pub const NOTES: &[u8] = str::NOTES.as_bytes();
    pub const ORIGIN: &[u8] = str::ORIGIN.as_bytes();
    pub const REFS: &[u8] = str::REFS.as_bytes();
    pub const REMOTES: &[u8] = str::REMOTES.as_bytes();
    pub const TAGS: &[u8] = str::TAGS.as_bytes();

    pub const REFS_HEADS_MAIN: &[u8] = str::REFS_HEADS_MAIN.as_bytes();
    pub const REFS_HEADS_MASTER: &[u8] = str::REFS_HEADS_MASTER.as_bytes();

    #[cfg(feature = "link-literals")]
    mod link {
        use super::str;

        pub const RAD: &[u8] = str::RAD.as_bytes();
        pub const ID: &[u8] = str::ID.as_bytes();
        pub const IDS: &[u8] = str::IDS.as_bytes();
        pub const SELF: &[u8] = str::SELF.as_bytes();
        pub const SIGNED_REFS: &[u8] = str::SIGNED_REFS.as_bytes();
        pub const COBS: &[u8] = str::COBS.as_bytes();

        pub const REFS_RAD_ID: &[u8] = str::REFS_RAD_ID.as_bytes();
        pub const REFS_RAD_SELF: &[u8] = str::REFS_RAD_SELF.as_bytes();
        pub const REFS_RAD_SIGNED_REFS: &[u8] = str::REFS_RAD_SIGNED_REFS.as_bytes();
    }
    #[cfg(feature = "link-literals")]
    pub use link::*;
}

#[cfg(feature = "link-literals")]
mod link {
    use super::{str, RefStr};

    pub const RAD: &RefStr = RefStr::from_str(str::RAD);
    pub const ID: &RefStr = RefStr::from_str(str::ID);
    pub const IDS: &RefStr = RefStr::from_str(str::IDS);
    pub const SELF: &RefStr = RefStr::from_str(str::SELF);
    pub const SIGNED_REFS: &RefStr = RefStr::from_str(str::SIGNED_REFS);
    pub const COBS: &RefStr = RefStr::from_str(str::COBS);

    pub const REFS_RAD_ID: &RefStr = RefStr::from_str(str::REFS_RAD_ID);
    pub const REFS_RAD_SELF: &RefStr = RefStr::from_str(str::REFS_RAD_SELF);
    pub const REFS_RAD_SIGNED_REFS: &RefStr = RefStr::from_str(str::REFS_RAD_SIGNED_REFS);
}
#[cfg(feature = "link-literals")]
pub use link::*;

const CHECK_OPTS: check::Options = check::Options {
    allow_pattern: false,
    allow_onelevel: true,
};

#[repr(transparent)]
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct RefStr(str);

impl RefStr {
    pub fn try_from_str(s: &str) -> Result<&RefStr, check::Error> {
        TryFrom::try_from(s)
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        self
    }

    #[inline]
    pub fn to_ref_string(&self) -> RefString {
        self.to_owned()
    }

    pub fn strip_prefix<P>(&self, base: P) -> Option<&RefStr>
    where
        P: AsRef<RefStr>,
    {
        self._strip_prefix(base.as_ref())
    }

    fn _strip_prefix(&self, base: &RefStr) -> Option<&RefStr> {
        self.0
            .strip_prefix(base.as_str())
            .and_then(|s| s.strip_prefix('/'))
            .map(Self::from_str)
    }

    /// Join `other` onto `self`, yielding a new [`RefString`].
    ///
    /// Consider to use [`RefString::and`] when chaining multiple fragments
    /// together, and the intermediate values are not needed.
    pub fn join<R>(&self, other: R) -> RefString
    where
        R: AsRef<RefStr>,
    {
        self._join(other.as_ref())
    }

    fn _join(&self, other: &RefStr) -> RefString {
        let mut buf = self.to_ref_string();
        buf.push(other);
        buf
    }

    pub fn to_pattern<P>(&self, pattern: P) -> PatternString
    where
        P: AsRef<PatternStr>,
    {
        self._to_pattern(pattern.as_ref())
    }

    fn _to_pattern(&self, pattern: &PatternStr) -> PatternString {
        self.to_owned().with_pattern(pattern)
    }

    #[inline]
    pub fn qualified(&self) -> Option<Qualified> {
        Qualified::from_refstr(self)
    }

    #[inline]
    pub fn namespaced(&self) -> Option<Namespaced> {
        self.into()
    }

    pub fn iter(&self) -> Iter {
        self.0.split('/')
    }

    pub fn components(&self) -> Components {
        Components::from(self)
    }

    pub fn head(&self) -> Component {
        self.components().next().expect("`RefStr` cannot be empty")
    }

    #[cfg(feature = "percent-encoding")]
    pub fn percent_encode(&self) -> PercentEncode {
        /// https://url.spec.whatwg.org/#fragment-percent-encode-set
        const FRAGMENT_PERCENT_ENCODE_SET: &percent_encoding::AsciiSet =
            &percent_encoding::CONTROLS
                .add(b' ')
                .add(b'"')
                .add(b'<')
                .add(b'>')
                .add(b'`');

        /// https://url.spec.whatwg.org/#path-percent-encode-set
        const PATH_PERCENT_ENCODE_SET: &percent_encoding::AsciiSet = &FRAGMENT_PERCENT_ENCODE_SET
            .add(b'#')
            .add(b'?')
            .add(b'{')
            .add(b'}');

        percent_encoding::utf8_percent_encode(self.as_str(), PATH_PERCENT_ENCODE_SET)
    }

    #[cfg(feature = "bstr")]
    #[inline]
    pub fn as_bstr(&self) -> &bstr::BStr {
        self.as_ref()
    }

    pub(crate) const fn from_str(s: &str) -> &RefStr {
        unsafe { &*(s as *const str as *const RefStr) }
    }
}

impl Deref for RefStr {
    type Target = str;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for RefStr {
    #[inline]
    fn as_ref(&self) -> &str {
        self
    }
}

#[cfg(feature = "bstr")]
impl AsRef<bstr::BStr> for RefStr {
    #[inline]
    fn as_ref(&self) -> &bstr::BStr {
        use bstr::ByteSlice as _;
        self.as_str().as_bytes().as_bstr()
    }
}

impl<'a> AsRef<RefStr> for &'a RefStr {
    #[inline]
    fn as_ref(&self) -> &RefStr {
        self
    }
}

impl<'a> TryFrom<&'a str> for &'a RefStr {
    type Error = check::Error;

    #[inline]
    fn try_from(s: &'a str) -> Result<Self, Self::Error> {
        check::ref_format(CHECK_OPTS, s).map(|()| RefStr::from_str(s))
    }
}

impl<'a> From<&'a RefStr> for Cow<'a, RefStr> {
    #[inline]
    fn from(rs: &'a RefStr) -> Cow<'a, RefStr> {
        Cow::Borrowed(rs)
    }
}

impl Display for RefStr {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self)
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct RefString(String);

impl RefString {
    #[inline]
    pub fn as_refstr(&self) -> &RefStr {
        self
    }

    /// Join `other` onto `self` in place.
    ///
    /// This is a consuming version of [`RefString::push`] which can be chained.
    /// Prefer this over chaining calls to [`RefStr::join`] if the
    /// intermediate values are not neede.
    pub fn and<R>(self, other: R) -> Self
    where
        R: AsRef<RefStr>,
    {
        self._and(other.as_ref())
    }

    fn _and(mut self, other: &RefStr) -> Self {
        self.push(other);
        self
    }

    pub fn push<R>(&mut self, other: R)
    where
        R: AsRef<RefStr>,
    {
        self.0.push('/');
        self.0.push_str(other.as_ref().as_str());
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

    /// Append a [`PatternStr`], turning self into a new [`PatternString`].
    pub fn with_pattern<P>(self, pattern: P) -> PatternString
    where
        P: AsRef<PatternStr>,
    {
        self._with_pattern(pattern.as_ref())
    }

    fn _with_pattern(self, pattern: &PatternStr) -> PatternString {
        let mut buf = self.0;
        buf.push('/');
        buf.push_str(pattern.as_str());

        PatternString(buf)
    }

    #[inline]
    pub fn into_qualified<'a>(self) -> Option<Qualified<'a>> {
        Qualified::from_refstr(self)
    }

    #[inline]
    pub fn reserve(&mut self, additional: usize) {
        self.0.reserve(additional)
    }

    #[inline]
    pub fn shrink_to_fit(&mut self) {
        self.0.shrink_to_fit()
    }

    #[cfg(feature = "bstr")]
    #[inline]
    pub fn into_bstring(self) -> bstr::BString {
        self.into()
    }

    #[cfg(feature = "bstr")]
    #[inline]
    pub fn as_bstr(&self) -> &bstr::BStr {
        self.as_ref()
    }
}

impl Deref for RefString {
    type Target = RefStr;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.borrow()
    }
}

impl AsRef<RefStr> for RefString {
    #[inline]
    fn as_ref(&self) -> &RefStr {
        self
    }
}

impl AsRef<str> for RefString {
    #[inline]
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

#[cfg(feature = "bstr")]
impl AsRef<bstr::BStr> for RefString {
    #[inline]
    fn as_ref(&self) -> &bstr::BStr {
        use bstr::ByteSlice as _;
        self.as_str().as_bytes().as_bstr()
    }
}

impl Borrow<RefStr> for RefString {
    #[inline]
    fn borrow(&self) -> &RefStr {
        RefStr::from_str(self.0.as_str())
    }
}

impl ToOwned for RefStr {
    type Owned = RefString;

    #[inline]
    fn to_owned(&self) -> Self::Owned {
        RefString(self.0.to_owned())
    }
}

impl TryFrom<&str> for RefString {
    type Error = check::Error;

    #[inline]
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        RefStr::try_from_str(s).map(ToOwned::to_owned)
    }
}

impl TryFrom<String> for RefString {
    type Error = check::Error;

    #[inline]
    fn try_from(s: String) -> Result<Self, Self::Error> {
        check::ref_format(CHECK_OPTS, s.as_str()).map(|()| RefString(s))
    }
}

impl<'a> From<&'a RefString> for Cow<'a, RefStr> {
    #[inline]
    fn from(rs: &'a RefString) -> Cow<'a, RefStr> {
        Cow::Borrowed(rs.as_refstr())
    }
}

impl<'a> From<RefString> for Cow<'a, RefStr> {
    #[inline]
    fn from(rs: RefString) -> Cow<'a, RefStr> {
        Cow::Owned(rs)
    }
}

impl From<RefString> for String {
    #[inline]
    fn from(rs: RefString) -> Self {
        rs.0
    }
}

#[cfg(feature = "bstr")]
impl From<RefString> for bstr::BString {
    #[inline]
    fn from(rs: RefString) -> Self {
        bstr::BString::from(rs.0.into_bytes())
    }
}

impl Display for RefString {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'a> FromIterator<&'a RefStr> for RefString {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = &'a RefStr>,
    {
        let mut buf = String::new();
        for x in iter {
            buf.push_str(x);
            buf.push('/');
        }
        buf.truncate(buf.len() - 1);
        assert!(!buf.is_empty(), "empty iterator");

        Self(buf)
    }
}

impl<'a> FromIterator<Component<'a>> for RefString {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = Component<'a>>,
    {
        let mut buf = String::new();
        for c in iter {
            buf.push_str(c.as_str());
            buf.push('/');
        }
        assert!(!buf.is_empty(), "empty iterator");
        buf.truncate(buf.len() - 1);

        Self(buf)
    }
}
