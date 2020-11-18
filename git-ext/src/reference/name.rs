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
    ffi::CString,
    fmt::{self, Display},
    iter::FromIterator,
    ops::Deref,
    path::{self, Path, PathBuf},
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
    NotPrefix(#[from] path::StripPrefixError),
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
#[derive(
    Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(into = "PathBuf", try_from = "PathBuf")]
pub struct RefLike(PathBuf);

impl RefLike {
    /// Append the path in `Other` to `self.
    pub fn join<Other: Into<Self>>(&self, other: Other) -> Self {
        Self(self.0.join(other.into().0))
    }

    /// Append a [`RefspecPattern`], yielding a [`RefspecPattern`]
    pub fn with_pattern_suffix<Suf: Into<RefspecPattern>>(&self, suf: Suf) -> RefspecPattern {
        RefspecPattern(self.0.join(suf.into().0))
    }

    /// Returns a [`RefLike`] that, when joined onto `base` (converted into
    /// [`Self`]), yields `self`.
    ///
    /// # Errors
    ///
    /// If `base` is not a prefix of `self`, or `base` equals the path in `self`
    /// (ie. the result would be the empty path, which is not a valid
    /// [`RefLike`]).
    pub fn strip_prefix<P: AsRef<Path>>(&self, base: P) -> Result<Self, StripPrefixError> {
        self.0
            .strip_prefix(base)
            .map_err(StripPrefixError::from)
            .and_then(|path| {
                if path.as_os_str().is_empty() {
                    Err(StripPrefixError::ImproperPrefix)
                } else {
                    Ok(Self(path.to_path_buf()))
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
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<Path> for RefLike {
    fn as_ref(&self) -> &Path {
        self
    }
}

impl AsRef<str> for RefLike {
    fn as_ref(&self) -> &str {
        self.0
            .to_str()
            .expect("cannot be constructed from invalid utf8")
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

impl TryFrom<PathBuf> for RefLike {
    type Error = Error;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        path.to_str().ok_or(Error::Utf8).and_then(Self::try_from)
    }
}

impl TryFrom<&Path> for RefLike {
    type Error = Error;

    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        path.to_str().ok_or(Error::Utf8).and_then(Self::try_from)
    }
}

impl From<&RefLike> for RefLike {
    fn from(me: &RefLike) -> Self {
        me.clone()
    }
}

impl From<RefLike> for PathBuf {
    fn from(RefLike(path): RefLike) -> Self {
        path
    }
}

impl FromIterator<Self> for RefLike {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = Self>,
    {
        Self(iter.into_iter().map(|x| x.0).collect())
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
/// use std::{convert::TryFrom, path::Path};
/// use radicle_git_ext::reference::name::*;
///
/// assert_eq!(
///     &*OneLevel::from(RefLike::try_from("refs/heads/next").unwrap()),
///     Path::new("next")
/// );
///
/// assert_eq!(
///     &*OneLevel::from(RefLike::try_from("refs/remotes/origin/it").unwrap()),
///     Path::new("origin/it")
/// );
///
/// assert_eq!(
///     &*OneLevel::from(RefLike::try_from("mistress").unwrap()),
///     Path::new("mistress")
/// );
/// ```
#[derive(
    Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(into = "PathBuf", try_from = "RefLike")]
pub struct OneLevel(PathBuf);

impl OneLevel {
    pub fn as_str(&self) -> &str {
        self.as_ref()
    }
}

impl Deref for OneLevel {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<Path> for OneLevel {
    fn as_ref(&self) -> &Path {
        self
    }
}

impl AsRef<str> for OneLevel {
    fn as_ref(&self) -> &str {
        self.0
            .to_str()
            .expect("cannot be constructed from invalid utf8")
    }
}

impl From<RefLike> for OneLevel {
    fn from(RefLike(path): RefLike) -> Self {
        if path.starts_with("refs/") {
            Self(path.iter().skip(2).collect())
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

impl From<OneLevel> for PathBuf {
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
/// use std::{convert::TryFrom, path::Path};
/// use radicle_git_ext::reference::name::*;
///
/// assert_eq!(
///     &*Qualified::from(RefLike::try_from("laplace").unwrap()),
///     Path::new("refs/heads/laplace")
/// );
///
/// assert_eq!(
///     &*Qualified::from(RefLike::try_from("refs/heads/pu").unwrap()),
///     Path::new("refs/heads/pu")
/// );
///
/// assert_eq!(
///     &*Qualified::from(RefLike::try_from("refs/tags/v6.6.6").unwrap()),
///     Path::new("refs/tags/v6.6.6")
/// );
/// ```
#[derive(
    Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(into = "PathBuf", try_from = "RefLike")]
pub struct Qualified(PathBuf);

impl Qualified {
    pub fn as_str(&self) -> &str {
        self.as_ref()
    }
}

impl Deref for Qualified {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<Path> for Qualified {
    fn as_ref(&self) -> &Path {
        self
    }
}

impl AsRef<str> for Qualified {
    fn as_ref(&self) -> &str {
        self.0
            .to_str()
            .expect("cannot be constructed from invalid utf8")
    }
}

impl From<RefLike> for Qualified {
    fn from(RefLike(path): RefLike) -> Self {
        if path.starts_with("refs/") {
            Self(path)
        } else {
            Self(Path::new("refs/heads").join(path))
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

impl From<Qualified> for PathBuf {
    fn from(Qualified(path): Qualified) -> Self {
        path
    }
}

impl Display for Qualified {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.as_str())
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
#[derive(
    Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(into = "PathBuf", try_from = "PathBuf")]
pub struct RefspecPattern(PathBuf);

impl RefspecPattern {
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
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<Path> for RefspecPattern {
    fn as_ref(&self) -> &Path {
        self
    }
}

impl AsRef<str> for RefspecPattern {
    fn as_ref(&self) -> &str {
        self.0
            .to_str()
            .expect("cannot be constructed from invalid utf8")
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

impl TryFrom<PathBuf> for RefspecPattern {
    type Error = Error;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        path.to_str().ok_or(Error::Utf8).and_then(Self::try_from)
    }
}

impl TryFrom<&Path> for RefspecPattern {
    type Error = Error;

    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        path.to_str().ok_or(Error::Utf8).and_then(Self::try_from)
    }
}

impl From<RefspecPattern> for PathBuf {
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

////////////////////////////////////////////////////////////////////////////////

fn normalize_name(s: &str, flags: git2::ReferenceFormat) -> Result<PathBuf, Error> {
    // FIXME(kim): libgit2 disagrees with git-check-ref-format on this one.
    // Submit patch upstream!
    if s == "@" {
        return Err(Error::RefFormat);
    }

    let nulsafe = CString::new(s)
        .map_err(|_| Error::Nul)?
        .into_string()
        .map_err(|_| Error::Utf8)?;

    git2::Reference::normalize_name(&nulsafe, flags)
        .map(PathBuf::from)
        .map_err(|e| match e.code() {
            git2::ErrorCode::InvalidSpec => Error::RefFormat,
            _ => Error::Git(e),
        })
}

////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;
    use librad_test::roundtrip::json_roundtrip;

    mod common {
        use super::*;
        use std::fmt::Debug;

        pub fn invalid<T>()
        where
            T: TryFrom<&'static str, Error = Error> + Debug,
        {
            const INVALID: &[&str] = &[
                "foo.lock",
                ".hidden",
                "here/../../etc/shadow",
                "/etc/shadow",
                "~ommij",
                "head^",
                "wh?t",
                "x[a-z]",
                "\\WORKGROUP",
                "C:",
                "@",
                "@{",
            ];

            for v in INVALID {
                assert_matches!(T::try_from(*v), Err(Error::RefFormat), "input: {}", v)
            }
        }

        pub fn valid<T>()
        where
            T: TryFrom<&'static str, Error = Error> + AsRef<str> + Debug,
        {
            const VALID: &[&str] = &[
                "master",
                "foo/bar",
                "cl@wn",
                "refs/heads/mistress",
                "\u{1F32F}",
            ];

            for v in VALID {
                assert_matches!(T::try_from(*v), Ok(ref x) if x.as_ref() == *v, "input: {}", v)
            }
        }

        pub fn empty<T>()
        where
            T: TryFrom<&'static str, Error = Error> + Debug,
        {
            assert_matches!(T::try_from(""), Err(Error::RefFormat))
        }

        pub fn nulsafe<T>()
        where
            T: TryFrom<&'static str, Error = Error> + Debug,
        {
            assert_matches!(T::try_from("jeff\0"), Err(Error::Nul))
        }

        pub fn normalises<T>()
        where
            T: TryFrom<&'static str, Error = Error> + AsRef<str> + Debug,
        {
            const SLASHED: &[&str] = &[
                "foo//bar",
                "foo//bar//baz",
                "refs//heads/main",
                "guns//////n/////roses",
            ];

            lazy_static! {
                static ref SLASHY: regex::Regex = regex::Regex::new(r"/{2,}").unwrap();
            }

            for v in SLASHED {
                let t = T::try_from(*v).unwrap();
                let normal = SLASHY.replace_all(v, "/");
                assert_eq!(t.as_ref(), &normal)
            }
        }
    }

    mod reflike {
        use super::*;

        #[test]
        fn empty() {
            common::empty::<RefLike>()
        }

        #[test]
        fn valid() {
            common::valid::<RefLike>()
        }

        #[test]
        fn invalid() {
            common::invalid::<RefLike>()
        }

        #[test]
        fn nulsafe() {
            common::nulsafe::<RefLike>()
        }

        #[test]
        fn normalises() {
            common::normalises::<RefLike>()
        }

        #[test]
        fn globstar_invalid() {
            assert_matches!(RefLike::try_from("refs/heads/*"), Err(Error::RefFormat))
        }

        #[test]
        fn into_onelevel() {
            assert_eq!(
                &*OneLevel::from(RefLike::try_from("refs/heads/next").unwrap()),
                Path::new("next")
            )
        }

        #[test]
        fn into_heads() {
            assert_eq!(
                &*Qualified::from(RefLike::try_from("pu").unwrap()),
                Path::new("refs/heads/pu")
            )
        }

        #[test]
        fn serde() {
            let refl = RefLike::try_from("pu").unwrap();
            json_roundtrip(refl.clone());
            json_roundtrip(OneLevel::from(refl.clone()));
            json_roundtrip(Qualified::from(refl))
        }

        #[test]
        fn serde_invalid() {
            let json = serde_json::to_string(Path::new("HEAD^")).unwrap();
            assert!(serde_json::from_str::<RefLike>(&json).is_err());
            assert!(serde_json::from_str::<OneLevel>(&json).is_err());
            assert!(serde_json::from_str::<Qualified>(&json).is_err())
        }
    }

    mod pattern {
        use super::*;

        #[test]
        fn empty() {
            common::empty::<RefspecPattern>()
        }

        #[test]
        fn valid() {
            common::valid::<RefspecPattern>()
        }

        #[test]
        fn invalid() {
            common::invalid::<RefspecPattern>()
        }

        #[test]
        fn nulsafe() {
            common::nulsafe::<RefspecPattern>()
        }

        #[test]
        fn normalises() {
            common::normalises::<RefspecPattern>()
        }

        #[test]
        fn globstar_ok() {
            const GLOBBED: &[&str] = &[
                "refs/heads/*",
                "refs/namespaces/*/refs/rad",
                "*",
                "foo/bar*",
                "foo*/bar",
            ];

            for v in GLOBBED {
                assert_matches!(
                    RefspecPattern::try_from(*v),
                    Ok(ref x) if x.as_str() == *v,
                    "input: {}", v
                )
            }
        }

        #[test]
        fn serde() {
            json_roundtrip(RefspecPattern::try_from("refs/heads/*").unwrap())
        }

        #[test]
        fn serde_invalid() {
            let json = serde_json::to_string(Path::new("HEAD^")).unwrap();
            assert!(serde_json::from_str::<RefspecPattern>(&json).is_err())
        }
    }
}
