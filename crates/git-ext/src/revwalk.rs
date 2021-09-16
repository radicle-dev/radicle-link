// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

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
