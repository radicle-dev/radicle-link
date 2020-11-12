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

use std::convert::TryFrom;

use git_ext::is_not_found_err;

use super::{
    super::{
        storage::{self, glob, Storage},
        types::Reference,
    },
    error::Error,
};
use crate::identities::{
    self,
    git::{Identities, SomeIdentity},
};

pub use identities::git::Urn;

/// Read an identity for which the type is not known statically.
///
/// Note that the [`Urn::path`] is honoured, and the identity is read from the
/// tip of the branch it resolves to. If that branch is not found, `None` is
/// returned.
#[tracing::instrument(level = "debug", skip(storage), err)]
pub fn get(storage: &Storage, urn: &Urn) -> Result<Option<SomeIdentity>, Error> {
    let branch = Reference::try_from(urn)?;
    tracing::trace!(
        "trying to resolve unknown identity at {} from {}",
        urn,
        branch
    );
    match storage.reference(&branch) {
        Ok(Some(reference)) => {
            let tip = reference.peel_to_commit()?.id();
            Ok(Some(identities(&storage).some_identity(tip)?))
        },

        Ok(None) => Ok(None),
        Err(storage::Error::Git(e)) if is_not_found_err(&e) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// List all identities found in `storage`.
#[tracing::instrument(level = "debug", skip(storage), err)]
pub fn list<'a>(
    storage: &'a Storage,
) -> Result<impl Iterator<Item = Result<SomeIdentity, Error>> + 'a, Error> {
    lazy_static! {
        static ref GLOB: glob::RefspecMatcher =
            refspec_pattern!("refs/namespaces/*/refs/rad/id").into();
    }

    let iter = storage
        .reference_names_glob(GLOB.clone())?
        .filter_map(move |name| match name {
            Ok(name) => {
                Urn::try_from(name).map_or(None, |urn| self::get(storage, &urn).transpose())
            },

            Err(e) => Some(Err(e.into())),
        });

    Ok(iter)
}

fn identities(storage: &Storage) -> Identities<!> {
    storage.identities()
}
