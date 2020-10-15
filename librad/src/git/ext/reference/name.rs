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
    ops::Deref,
    path::{Path, PathBuf},
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
pub struct RefLike(PathBuf);

impl RefLike {
    pub fn as_str(&self) -> &str {
        self.into()
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

impl<'a> From<&'a RefLike> for &'a str {
    fn from(reflike: &'a RefLike) -> &'a str {
        reflike
            .0
            .to_str()
            .expect("cannot be constructed from invalid utf8")
    }
}

/// A [`RefLike`] without a "refs/" prefix.
///
/// Conversion functions strip the first **two** path components iff the path
/// starts with `refs/`.
///
/// # Examples
///
/// ```rust
/// use std::{convert::TryFrom, path::Path};
/// use librad::git::ext::reference::name::*;
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
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct OneLevel(PathBuf);

impl Deref for OneLevel {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
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

/// A [`RefLike`] **with** a "refs/" prefix.
///
/// Conversion functions will assume `refs/heads/` if the input was not
/// qualified.
///
/// # Examples
///
/// ```rust
/// use std::{convert::TryFrom, path::Path};
/// use librad::git::ext::reference::name::*;
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
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct Qualified(PathBuf);

impl Deref for Qualified {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
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

/// An owned, path-like value which is a valid refspec pattern.
///
/// Conversion functions behave as if `--normalize --allow-onelevel
/// --refspec-pattern` where given to [`git-check-ref-format`]. That is, most of
/// the rules of [`RefLike`] apply, but the path _may_ contain exactly one `*`
/// character.
///
/// [`git-check-ref-format`]: https://git-scm.com/docs/git-check-ref-format
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct RefspecPattern(PathBuf);

impl RefspecPattern {
    pub fn as_str(&self) -> &str {
        self.into()
    }
}

impl Deref for RefspecPattern {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
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

impl<'a> From<&'a RefspecPattern> for &'a str {
    fn from(refpat: &'a RefspecPattern) -> &'a str {
        refpat
            .0
            .to_str()
            .expect("cannot be constructed from invalid utf8")
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
            T: TryFrom<&'static str, Error = Error> + Debug,
            for<'a> &'a T: Into<&'a str>,
        {
            const VALID: &[&str] = &[
                "master",
                "foo/bar",
                "cl@wn",
                "refs/heads/mistress",
                "\u{1F32F}",
            ];

            for v in VALID {
                assert_matches!(T::try_from(*v), Ok(ref x) if x.into() == *v, "input: {}", v)
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
            T: TryFrom<&'static str, Error = Error> + Debug,
            for<'a> &'a T: Into<&'a str>,
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
                assert_eq!((&t).into(), &normal)
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
    }
}
