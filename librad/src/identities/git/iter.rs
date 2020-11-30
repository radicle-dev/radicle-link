// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

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
