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

pub enum Start {
    Oid(git2::Oid),
    Ref(String),
}

pub struct FirstParent<'a> {
    inner: git2::Revwalk<'a>,
}

impl<'a> FirstParent<'a> {
    pub fn new(repo: &'a git2::Repository, start: Start) -> Result<Self, git2::Error> {
        let mut revwalk = repo.revwalk()?;
        revwalk.set_sorting(git2::Sort::TOPOLOGICAL)?;
        revwalk.simplify_first_parent()?;

        match start {
            Start::Oid(oid) => revwalk.push(oid),
            Start::Ref(name) => revwalk.push_ref(&name),
        }?;

        Ok(Self { inner: revwalk })
    }

    pub fn reverse(mut self) -> Result<Self, git2::Error> {
        let mut sort = git2::Sort::TOPOLOGICAL;
        sort.insert(git2::Sort::REVERSE);
        self.inner.set_sorting(sort)?;
        Ok(self)
    }
}

impl<'a> IntoIterator for FirstParent<'a> {
    type Item = Result<git2::Oid, git2::Error>;
    type IntoIter = git2::Revwalk<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner
    }
}
