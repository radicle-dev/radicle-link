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
    ops::Deref,
    path::{Path, PathBuf},
    str::{self, FromStr},
};

use regex::RegexSet;
use thiserror::Error;

use crate::internal::borrow::TryToOwned;

pub use percent_encoding::PercentEncode;

/// Iterator chaining multiple [`git2::References`]
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct References<'a> {
    inner: Vec<git2::References<'a>>,
}

impl<'a> References<'a> {
    pub fn new(refs: impl IntoIterator<Item = git2::References<'a>>) -> Self {
        Self {
            inner: refs.into_iter().collect(),
        }
    }

    pub fn from_globs(
        repo: &'a git2::Repository,
        globs: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<Self, git2::Error> {
        let globs = globs.into_iter();
        let mut iters = globs
            .size_hint()
            .1
            .map(Vec::with_capacity)
            .unwrap_or_else(Vec::new);
        for glob in globs {
            let iter = repo.references_glob(glob.as_ref())?;
            iters.push(iter);
        }

        Ok(Self::new(iters))
    }

    pub fn names<'b>(&'b mut self) -> ReferenceNames<'a, 'b> {
        ReferenceNames {
            inner: self.inner.iter_mut().map(|refs| refs.names()).collect(),
        }
    }

    pub fn peeled(self) -> impl Iterator<Item = (String, git2::Oid)> + 'a {
        self.filter_map(|reference| {
            reference.ok().and_then(|head| {
                head.name().and_then(|name| {
                    head.target()
                        .map(|target| (name.to_owned(), target.to_owned()))
                })
            })
        })
    }
}

impl<'a> Iterator for References<'a> {
    type Item = Result<git2::Reference<'a>, git2::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.pop().and_then(|mut iter| match iter.next() {
            None => self.next(),
            Some(item) => {
                self.inner.push(iter);
                Some(item)
            },
        })
    }
}

/// Iterator chaining multiple [`git2::ReferenceNames`]
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct ReferenceNames<'repo, 'references> {
    inner: Vec<git2::ReferenceNames<'repo, 'references>>,
}

impl<'a, 'b> Iterator for ReferenceNames<'a, 'b> {
    type Item = Result<&'b str, git2::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.pop().and_then(|mut iter| match iter.next() {
            None => self.next(),
            Some(item) => {
                self.inner.push(iter);
                Some(item)
            },
        })
    }
}

impl TryToOwned for git2::Repository {
    type Owned = git2::Repository;
    type Error = git2::Error;

    fn try_to_owned(&self) -> Result<Self::Owned, Self::Error> {
        git2::Repository::open(self.path())
    }
}

/// An owned path-like value which is a valid git refname.
///
/// See [`git-check-ref-format`] for what the rules for refnames are.
/// Additionally, we impose the rule that the name must consist of valid utf8.
///
/// Note that refspec patterns (eg. "refs/heads/*") are not allowed, and that
/// the maximum length of the name is 1024 bytes.
///
/// [`git-check-ref-format`]: https://git-scm.com/docs/git-check-ref-format
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash, serde::Serialize)]
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

    #[allow(clippy::trivial_regex)]
    fn guard_valid(s: &str) -> Result<(), InvalidRefLike> {
        // FIXME(kim): replace with `git2::Reference::normalize_name` after
        // rust-lang/git2-rs#620. Their implementation matters, not what the
        // docs say.
        lazy_static! {
            static ref REFERENCE_FORMAT_RE: RegexSet = RegexSet::new(&[
                r"^$",
                r"\.lock$",
                r"^\.",
                r"\.\.",
                r"[[:cntrl:]]",
                r"[~^:?*\[\\]",
                r"@[{]",
                r"^/",
                r"//",
                r"^@$"
            ])
            .unwrap();
        }

        if s.len() > 1024 || REFERENCE_FORMAT_RE.is_match(s) {
            Err(InvalidRefLike::RefFormat)
        } else {
            Ok(())
        }
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

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum InvalidRefLike {
    #[error("invalid utf8")]
    Utf8,

    #[error("not a valid git ref name")]
    RefFormat,
}

impl TryFrom<&[u8]> for RefLike {
    type Error = InvalidRefLike;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        str::from_utf8(bytes)
            .or(Err(InvalidRefLike::Utf8))
            .and_then(Self::try_from)
    }
}

impl TryFrom<&str> for RefLike {
    type Error = InvalidRefLike;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::guard_valid(s)?;
        Ok(Self(s.into()))
    }
}

impl FromStr for RefLike {
    type Err = InvalidRefLike;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s)
    }
}

impl TryFrom<String> for RefLike {
    type Error = InvalidRefLike;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::try_from(s.as_str())
    }
}

impl TryFrom<PathBuf> for RefLike {
    type Error = InvalidRefLike;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        path.to_str()
            .ok_or(InvalidRefLike::Utf8)
            .and_then(Self::try_from)
    }
}

impl TryFrom<&Path> for RefLike {
    type Error = InvalidRefLike;

    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        path.to_str()
            .ok_or(InvalidRefLike::Utf8)
            .and_then(Self::try_from)
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

#[cfg(test)]
mod tests {
    use super::*;

    use pretty_assertions::assert_eq;

    #[test]
    fn empty_reflike() {
        assert_matches!(RefLike::try_from(""), Err(InvalidRefLike::RefFormat))
    }

    #[test]
    fn assorted_invalid_reflike() {
        [
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
            "foo//bar",
            "@",
            "@{",
            "refs/heads/*",
            "jeff\0",
        ]
        .iter()
        .for_each(|v| assert_matches!(RefLike::try_from(*v), Err(InvalidRefLike::RefFormat)))
    }

    #[test]
    fn assorted_valid_reflike() {
        ["master", "cl@wn", "refs/heads/mistress", "\u{1F32F}"]
            .iter()
            .for_each(|v| {
                let reflike = RefLike::try_from(*v);
                assert_matches!(reflike, Ok(_), "input: {}", v);
                let reflike = reflike.unwrap();
                assert_eq!(reflike.as_str(), *v);
            })
    }
}
