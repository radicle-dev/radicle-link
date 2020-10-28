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

use radicle_git_ext::is_not_found_err;

use super::error::Error;
use crate::{
    git::{
        storage2::{self, Storage},
        types::Reference,
    },
    identities::{self, git::SomeIdentity},
    signer::Signer,
};

pub use identities::git::Urn;

pub fn get<S>(storage: &Storage<S>, urn: &Urn) -> Result<Option<SomeIdentity>, Error>
where
    S: Signer,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    match storage.reference(&Reference::try_from(urn)?) {
        Ok(Some(reference)) => {
            let tip = reference.peel_to_commit()?.id();
            Ok(Some(storage.identities::<'_, !>().some_identity(tip)?))
        },

        Ok(None) => Ok(None),
        Err(storage2::Error::Git(e)) if is_not_found_err(&e) => Ok(None),
        Err(e) => Err(e.into()),
    }
}
