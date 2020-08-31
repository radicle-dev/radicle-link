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

use std::{convert::TryFrom, marker::PhantomData};

use crate::identities::generic::{self, Untrusted};

use super::{error, ByOid};

#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct Iter<'a, T> {
    repo: &'a git2::Repository,
    iter: git2::Revwalk<'a>,
    _marker: PhantomData<T>,
}

impl<'a, T> Iter<'a, T> {
    pub fn new(repo: &'a git2::Repository, head: git2::Oid) -> Result<Self, error::Load> {
        let mut revwalk = repo.revwalk()?;
        revwalk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::REVERSE)?;
        revwalk.simplify_first_parent()?;
        revwalk.push(head)?;

        Ok(Self {
            repo,
            iter: revwalk,
            _marker: PhantomData,
        })
    }
}

impl<'a, T> Iterator for Iter<'a, T>
where
    T: TryFrom<ByOid<'a>, Error = error::Load>,
{
    type Item = Result<generic::Verifying<T, Untrusted>, error::Load>;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter
            .next()
            .map(|oid| T::try_from((self.repo, oid?)).map(generic::Verifying::from))
    }
}
