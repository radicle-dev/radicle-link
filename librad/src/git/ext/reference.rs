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

use crate::internal::borrow::TryToOwned;

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
