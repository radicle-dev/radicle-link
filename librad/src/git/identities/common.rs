// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use git_ext::is_exists_err;
use std_ext::result::ResultExt as _;

use super::super::{
    storage::Storage,
    types::{Force, Namespace, Reference},
};
use crate::identities::git::Urn;

/// Ad-hoc helper type for conveniently managing `rad/id` refs
pub struct IdRef<'a>(&'a Urn);

impl<'a> From<&'a Urn> for IdRef<'a> {
    fn from(urn: &'a Urn) -> Self {
        Self(urn)
    }
}

impl<'a> IdRef<'a> {
    pub fn oid(&self, storage: &Storage) -> Result<git2::Oid, git2::Error> {
        Reference::rad_id(Namespace::from(self.0)).oid(storage.as_raw())
    }

    pub fn create(
        &self,
        storage: &Storage,
        target: impl AsRef<git2::Oid>,
    ) -> Result<(), git2::Error> {
        Reference::rad_id(Namespace::from(self.0))
            .create(
                storage.as_raw(),
                *target.as_ref(),
                Force::False,
                &format!("Initial rad/id for {}", self.0),
            )
            .and(Ok(()))
            .or_matches(is_exists_err, || Ok(()))
    }

    pub fn update(
        &self,
        storage: &Storage,
        target: impl AsRef<git2::Oid>,
        msg: &str,
    ) -> Result<(), git2::Error> {
        Reference::rad_id(Namespace::from(self.0))
            .create(storage.as_raw(), *target.as_ref(), Force::True, msg)
            .and(Ok(()))
    }
}
