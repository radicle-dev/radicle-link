// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    convert::TryFrom,
    ffi::CString,
    fmt::{self, Display},
    iter::FromIterator,
    ops::Deref,
    str::{self, FromStr},
};

pub use percent_encoding::PercentEncode;
use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("invalid utf8")]
    Utf8,

    #[error("not a valid git ref name or pattern")]
    RefFormat,

    #[error("input contains a nul byte")]
    Nul,

    #[error(transparent)]
    Git(#[from] git2::Error),
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StripPrefixError {
    #[error("prefix is equal to path")]
    ImproperPrefix,

    #[error("not prefixed by given path")]
    NotPrefix,
}

/// An owned path-like value which is a valid git refname.
///
/// See [`git-check-ref-format`] for what the rules for refnames are --
/// conversion functions behave as if `--normalize --allow-onelevel` was given.
/// Additionally, we impose the rule that the name must consist of valid utf8.
///
/// Note that refspec patterns (eg. "refs/heads/*") are not allowed (see
/// [`RefspecPattern`]), and that the maximum length of the name is 1024 bytes.
///
/// [`git-check-ref-format`]: https://git-scm.com/docs/git-check-ref-format
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(into = "String", try_from = "String")
)]
pub struct RefLike(String);

impl RefLike {
    /// Append the path in `Other` to `self.
    pub fn join<Other: Into<Self>>(&self, other: Other) -> Self {
        Self(format!("{}/{}", self.0, other.into().0))
    }

    /// Append a [`RefspecPattern`], yielding a [`RefspecPattern`]
    pub fn with_pattern_suffix<Suf: Into<RefspecPattern>>(&self, suf: Suf) -> RefspecPattern {
        RefspecPattern(format!("{}/{}", self.0, suf.into().0))
    }

    /// Returns a [`RefLike`] that, when joined onto `base`, yields `self`.
    ///
    /// # Errors
    ///
    /// If `base` is not a prefix of `self`, or `base` equals the path in `self`
    /// (ie. the result would be the empty path, which is not a valid
    /// [`RefLike`]).
    pub fn strip_prefix<P: AsRef<str>>(&self, base: P) -> Result<Self, StripPrefixError> {
        let base = base.as_ref();
        let base = format!("{}/", base.strip_suffix("/").unwrap_or(base));
        self.0
            .strip_prefix(&base)
            .ok_or(StripPrefixError::NotPrefix)
            .and_then(|path| {
                if path.is_empty() {
                    Err(StripPrefixError::ImproperPrefix)
                } else {
                    Ok(Self(path.into()))
                }
            })
    }

    pub fn as_str(&self) -> &str {
        self.as_ref()
    }

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
}

impl Deref for RefLike {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for RefLike {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for RefLike {
    type Error = Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        normalize_name(
            s,
            git2::ReferenceFormat::ALLOW_ONELEVEL | git2::ReferenceFormat::REFSPEC_SHORTHAND,
        )
        .map(Self)
    }
}

impl TryFrom<&[u8]> for RefLike {
    type Error = Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        str::from_utf8(bytes)
            .or(Err(Error::Utf8))
            .and_then(Self::try_from)
    }
}

impl FromStr for RefLike {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s)
    }
}

impl TryFrom<String> for RefLike {
    type Error = Error;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::try_from(s.as_str())
    }
}

impl From<&RefLike> for RefLike {
    fn from(me: &RefLike) -> Self {
        me.clone()
    }
}

impl From<RefLike> for String {
    fn from(RefLike(path): RefLike) -> Self {
        path
    }
}

impl FromIterator<Self> for RefLike {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = Self>,
    {
        Self(iter.into_iter().map(|x| x.0).collect::<Vec<_>>().join("/"))
    }
}

impl Display for RefLike {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A [`RefLike`] without a "refs/" prefix.
///
/// Conversion functions strip the first **two** path components iff the path
/// starts with `refs/`.
///
/// Note that the [`serde::Deserialize`] impl thusly implies that input in
/// [`Qualified`] form is accepted, and silently converted.
///
/// # Examples
///
/// ```rust
/// use std::convert::TryFrom;
/// use radicle_git_ext::reference::name::*;
///
/// assert_eq!(
///     &*OneLevel::from(RefLike::try_from("refs/heads/next").unwrap()),
///     "next"
/// );
///
/// assert_eq!(
///     &*OneLevel::from(RefLike::try_from("refs/remotes/origin/it").unwrap()),
///     "origin/it"
/// );
///
/// assert_eq!(
///     &*OneLevel::from(RefLike::try_from("mistress").unwrap()),
///     "mistress"
/// );
///
/// assert_eq!(
///     OneLevel::from_qualified(Qualified::from(RefLike::try_from("refs/tags/grace").unwrap())),
///     (
///         OneLevel::from(RefLike::try_from("grace").unwrap()),
///         Some(RefLike::try_from("tags").unwrap())
///     ),
/// );
///
/// assert_eq!(
///     OneLevel::from_qualified(Qualified::from(RefLike::try_from("refs/remotes/origin/hopper").unwrap())),
///     (
///         OneLevel::from(RefLike::try_from("origin/hopper").unwrap()),
///         Some(RefLike::try_from("remotes").unwrap())
///     ),
/// );
///
/// assert_eq!(
///     OneLevel::from_qualified(Qualified::from(RefLike::try_from("refs/HEAD").unwrap())),
///     (OneLevel::from(RefLike::try_from("HEAD").unwrap()), None)
/// );
///
/// assert_eq!(
///     &*OneLevel::from(RefLike::try_from("origin/hopper").unwrap()).into_qualified(
///         RefLike::try_from("remotes").unwrap()
///     ),
///     "refs/remotes/origin/hopper",
/// );
/// ```
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(into = "String", try_from = "RefLike")
)]
pub struct OneLevel(String);

impl OneLevel {
    pub fn as_str(&self) -> &str {
        self.as_ref()
    }

    pub fn from_qualified(Qualified(path): Qualified) -> (Self, Option<RefLike>) {
        let mut path = path.strip_prefix("refs/").unwrap_or(&path).split('/');
        match path.next() {
            Some(category) => {
                let category = RefLike(category.into());
                // check that the "category" is not the only component of the path
                match path.next() {
                    Some(head) => (
                        Self(
                            std::iter::once(head)
                                .chain(path)
                                .collect::<Vec<_>>()
                                .join("/"),
                        ),
                        Some(category),
                    ),
                    None => (Self::from(category), None),
                }
            },
            None => unreachable!(),
        }
    }

    pub fn into_qualified(self, category: RefLike) -> Qualified {
        Qualified(format!("refs/{}/{}", category, self))
    }
}

impl Deref for OneLevel {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for OneLevel {
    fn as_ref(&self) -> &str {
        self
    }
}

impl From<RefLike> for OneLevel {
    fn from(RefLike(path): RefLike) -> Self {
        if path.starts_with("refs/") {
            Self(path.split('/').skip(2).collect::<Vec<_>>().join("/"))
        } else {
            Self(path)
        }
    }
}

impl From<Qualified> for OneLevel {
    fn from(Qualified(path): Qualified) -> Self {
        Self::from(RefLike(path))
    }
}

impl From<OneLevel> for RefLike {
    fn from(OneLevel(path): OneLevel) -> Self {
        Self(path)
    }
}

impl From<OneLevel> for String {
    fn from(OneLevel(path): OneLevel) -> Self {
        path
    }
}

impl Display for OneLevel {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A [`RefLike`] **with** a "refs/" prefix.
///
/// Conversion functions will assume `refs/heads/` if the input was not
/// qualified.
///
/// Note that the [`serde::Deserialize`] impl thusly implies that input in
/// [`OneLevel`] form is accepted, and silently converted.
///
/// # Examples
///
/// ```rust
/// use std::convert::TryFrom;
/// use radicle_git_ext::reference::name::*;
///
/// assert_eq!(
///     &*Qualified::from(RefLike::try_from("laplace").unwrap()),
///     "refs/heads/laplace"
/// );
///
/// assert_eq!(
///     &*Qualified::from(RefLike::try_from("refs/heads/pu").unwrap()),
///     "refs/heads/pu"
/// );
///
/// assert_eq!(
///     &*Qualified::from(RefLike::try_from("refs/tags/v6.6.6").unwrap()),
///     "refs/tags/v6.6.6"
/// );
/// ```
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(into = "String", try_from = "RefLike")
)]
pub struct Qualified(String);

impl Qualified {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Deref for Qualified {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for Qualified {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<RefLike> for Qualified {
    fn from(RefLike(path): RefLike) -> Self {
        if path.starts_with("refs/") {
            Self(path)
        } else {
            Self(format!("refs/heads/{}", path))
        }
    }
}

impl From<OneLevel> for Qualified {
    fn from(OneLevel(path): OneLevel) -> Self {
        Self::from(RefLike(path))
    }
}

impl From<Qualified> for RefLike {
    fn from(Qualified(path): Qualified) -> Self {
        Self(path)
    }
}

impl From<Qualified> for String {
    fn from(Qualified(path): Qualified) -> Self {
        path
    }
}

impl Display for Qualified {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self)
    }
}

/// An owned, path-like value which is a valid refspec pattern.
///
/// Conversion functions behave as if `--normalize --allow-onelevel
/// --refspec-pattern` where given to [`git-check-ref-format`]. That is, most of
/// the rules of [`RefLike`] apply, but the path _may_ contain exactly one `*`
/// character.
///
/// [`git-check-ref-format`]: https://git-scm.com/docs/git-check-ref-format
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(into = "String", try_from = "String")
)]
pub struct RefspecPattern(String);

impl RefspecPattern {
    /// Append the `RefLike` to the `RefspecPattern`. This allows the creation
    /// of patterns where the `*` appears in the middle of the path, e.g.
    /// `refs/remotes/*/mfdoom`
    pub fn append(&self, refl: impl Into<RefLike>) -> Self {
        RefspecPattern(format!("{}/{}", self.0, refl.into()))
    }

    pub fn as_str(&self) -> &str {
        self.as_ref()
    }
}

impl From<&RefspecPattern> for RefspecPattern {
    fn from(pat: &RefspecPattern) -> Self {
        pat.clone()
    }
}

impl Deref for RefspecPattern {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for RefspecPattern {
    fn as_ref(&self) -> &str {
        self
    }
}

impl TryFrom<&str> for RefspecPattern {
    type Error = Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        normalize_name(
            s,
            git2::ReferenceFormat::ALLOW_ONELEVEL
                | git2::ReferenceFormat::REFSPEC_SHORTHAND
                | git2::ReferenceFormat::REFSPEC_PATTERN,
        )
        .map(Self)
    }
}

impl TryFrom<&[u8]> for RefspecPattern {
    type Error = Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        str::from_utf8(bytes)
            .or(Err(Error::Utf8))
            .and_then(Self::try_from)
    }
}

impl FromStr for RefspecPattern {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s)
    }
}

impl TryFrom<String> for RefspecPattern {
    type Error = Error;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::try_from(s.as_str())
    }
}

impl From<RefspecPattern> for String {
    fn from(RefspecPattern(path): RefspecPattern) -> Self {
        path
    }
}

impl Display for RefspecPattern {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// `RefLike`-likes can be coerced into `RefspecPattern`s

impl From<RefLike> for RefspecPattern {
    fn from(RefLike(path): RefLike) -> Self {
        Self(path)
    }
}

impl From<&RefLike> for RefspecPattern {
    fn from(RefLike(path): &RefLike) -> Self {
        Self(path.to_owned())
    }
}

impl From<OneLevel> for RefspecPattern {
    fn from(OneLevel(path): OneLevel) -> Self {
        Self(path)
    }
}

impl From<&OneLevel> for RefspecPattern {
    fn from(OneLevel(path): &OneLevel) -> Self {
        Self(path.to_owned())
    }
}

impl From<Qualified> for RefspecPattern {
    fn from(Qualified(path): Qualified) -> Self {
        Self(path)
    }
}

impl From<&Qualified> for RefspecPattern {
    fn from(Qualified(path): &Qualified) -> Self {
        Self(path.to_owned())
    }
}

#[cfg(feature = "minicbor")]
mod minicbor_impls {
    use super::*;
    use minicbor::{
        decode,
        encode::{self, Write},
        Decode,
        Decoder,
        Encode,
        Encoder,
    };

    impl Encode for RefLike {
        fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), encode::Error<W::Error>> {
            e.str(self.as_str())?;
            Ok(())
        }
    }

    impl<'b> Decode<'b> for RefLike {
        fn decode(d: &mut Decoder) -> Result<Self, decode::Error> {
            let path = d.str()?;
            Self::try_from(path).or(Err(decode::Error::Message("invalid reflike")))
        }
    }

    impl minicbor::Encode for OneLevel {
        fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), encode::Error<W::Error>> {
            e.str(self.as_str())?;
            Ok(())
        }
    }

    impl<'b> Decode<'b> for OneLevel {
        fn decode(d: &mut Decoder) -> Result<Self, decode::Error> {
            let refl: RefLike = Decode::decode(d)?;
            Ok(Self::from(refl))
        }
    }

    impl Encode for Qualified {
        fn encode<W: encode::Write>(
            &self,
            e: &mut Encoder<W>,
        ) -> Result<(), encode::Error<W::Error>> {
            e.str(self.as_str())?;
            Ok(())
        }
    }

    impl<'b> Decode<'b> for Qualified {
        fn decode(d: &mut Decoder) -> Result<Self, decode::Error> {
            let refl: RefLike = Decode::decode(d)?;
            Ok(Self::from(refl))
        }
    }

    impl Encode for RefspecPattern {
        fn encode<W: encode::Write>(
            &self,
            e: &mut Encoder<W>,
        ) -> Result<(), encode::Error<W::Error>> {
            e.str(self.as_str())?;
            Ok(())
        }
    }

    impl<'b> Decode<'b> for RefspecPattern {
        fn decode(d: &mut Decoder) -> Result<Self, decode::Error> {
            Self::try_from(d.str()?).or(Err(decode::Error::Message("invalid refspec pattern")))
        }
    }
}

////////////////////////////////////////////////////////////////////////////////

fn normalize_name(s: &str, flags: git2::ReferenceFormat) -> Result<String, Error> {
    // FIXME(kim): libgit2 disagrees with git-check-ref-format on this one.
    // Submit patch upstream!
    if s == "@" {
        return Err(Error::RefFormat);
    }

    let nulsafe = CString::new(s)
        .map_err(|_| Error::Nul)?
        .into_string()
        .map_err(|_| Error::Utf8)?;

    git2::Reference::normalize_name(&nulsafe, flags).map_err(|e| match e.code() {
        git2::ErrorCode::InvalidSpec => Error::RefFormat,
        _ => Error::Git(e),
    })
}

////////////////////////////////////////////////////////////////////////////////
