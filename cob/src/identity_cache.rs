// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use link_identities::{
    error::Load,
    git::{Person, Project, SomeIdentity},
    Identities,
};

use either::Either;
use std::collections::HashMap;
use thiserror::Error;

pub(crate) struct IdentityCache<'a> {
    repo: &'a git2::Repository,
    cached_identities: HashMap<git2::Oid, Either<Person, Project>>,
}

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Identities(#[from] Load),
}

impl<'a> IdentityCache<'a> {
    pub(crate) fn new(repo: &'a git2::Repository) -> IdentityCache<'a> {
        IdentityCache {
            repo,
            cached_identities: HashMap::new(),
        }
    }

    pub(crate) fn lookup_identity(
        &mut self,
        commit: git2::Oid,
    ) -> Result<Option<&Either<Person, Project>>, Error> {
        let identities: Identities<'_, SomeIdentity> = self.repo.into();
        let result = match identities.some_identity(commit)? {
            SomeIdentity::Person(p) => Either::Left(p),
            SomeIdentity::Project(p) => Either::Right(p),
            _ => return Ok(None),
        };
        self.cached_identities.insert(commit, result);
        Ok(self.cached_identities.get(&commit))
    }
}
