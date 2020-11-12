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

use git_ext::is_exists_err;
use std_ext::result::ResultExt as _;

use super::super::{
    storage::Storage,
    types::{namespace::Namespace, Force, NamespacedRef},
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
    pub fn create(
        &self,
        storage: &Storage,
        target: impl AsRef<git2::Oid>,
    ) -> Result<(), git2::Error> {
        NamespacedRef::rad_id(Namespace::from(self.0))
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
        NamespacedRef::rad_id(Namespace::from(self.0))
            .create(storage.as_raw(), *target.as_ref(), Force::True, msg)
            .and(Ok(()))
    }
}
