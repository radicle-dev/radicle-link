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

use std::{convert::TryFrom, fmt::Debug};

use radicle_git_ext::is_not_found_err;

use super::{
    super::{
        storage::{self, Storage},
        types::Reference,
    },
    common,
    error::Error,
    local::LocalIdentity,
};
use crate::{
    identities::{
        self,
        delegation,
        git::{Identities, Verifying},
        urn,
    },
    peer::PeerId,
    signer::Signer,
};

pub use identities::{
    git::{Urn, User, VerifiedUser},
    payload::UserPayload,
};

/// Read a [`User`] from the tip of thr ref [`Urn::path`] points to.
///
/// If the ref is not found, `None` is returned.
#[tracing::instrument(level = "trace", skip(storage), err)]
pub fn get<S>(storage: &Storage<S>, urn: &Urn) -> Result<Option<User>, Error>
where
    S: Signer,
{
    match storage.reference(&Reference::try_from(urn)?) {
        Ok(Some(reference)) => {
            let tip = reference.peel_to_commit()?.id();
            Ok(Some(identities(storage).get(tip)?))
        },

        Ok(None) => Ok(None),
        Err(storage::Error::Git(e)) if is_not_found_err(&e) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Read and verify the [`User`] pointed to by `urn`.
///
/// If the ref pointed to by [`Urn::path`] is not found, `None` is returned.
///
/// # Caveats
///
/// Keep in mind that the `content_id` of a successfully verified user may
/// not be the same as the tip of the ref [`Urn::path`] points to. That is, this
/// function cannot be used to assert that the state after an [`update`] is
/// valid.
#[tracing::instrument(level = "debug", skip(storage), err)]
pub fn verify<S>(storage: &Storage<S>, urn: &Urn) -> Result<Option<VerifiedUser>, Error>
where
    S: Signer,
{
    match storage.reference(&Reference::try_from(urn)?) {
        Ok(Some(reference)) => {
            let tip = reference.peel_to_commit()?.id();
            identities(storage)
                .verify(tip)
                .map(Some)
                .map_err(|e| Error::Verify(e.into()))
        },

        Ok(None) => Ok(None),
        Err(storage::Error::Git(e)) if is_not_found_err(&e) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Create a new [`User`].
///
/// The `delegations` must include the [`Signer`] key, such that the newly
/// created [`User`] is also a valid [`LocalIdentity`] -- it is, in fact, its
/// own [`LocalIdentity`]. This can be changed via [`update`].
#[tracing::instrument(level = "debug", skip(storage), err)]
pub fn create<S, P>(
    storage: &Storage<S>,
    payload: P,
    delegations: delegation::Direct,
) -> Result<User, Error>
where
    S: Signer,
    P: Into<UserPayload> + Debug,
{
    let user = {
        let user = identities(storage).create(payload.into(), delegations, storage.signer())?;
        let verified = identities(storage)
            .verify(*user.content_id)
            .map_err(|e| Error::Verify(e.into()))?;
        LocalIdentity::valid(verified, storage.signer())
    }?;

    let urn = user.urn();
    common::IdRef::from(&urn).create(storage, user.content_id)?;
    user.link(storage, &urn)?;

    Ok(user.into_inner().into_inner())
}

/// Update the [`User`] at `urn`.
#[tracing::instrument(level = "debug", skip(storage), err)]
pub fn update<S, L, P, D>(
    storage: &Storage<S>,
    urn: &Urn,
    whoami: L,
    payload: P,
    delegations: D,
) -> Result<User, Error>
where
    S: Signer,
    L: Into<Option<LocalIdentity>> + Debug,
    P: Into<Option<UserPayload>> + Debug,
    D: Into<Option<delegation::Direct>> + Debug,
{
    let prev = get(storage, urn)?.ok_or_else(|| Error::NotFound(urn.clone()))?;
    let prev = Verifying::from(prev).signed()?;
    let next = identities(storage).update(prev, payload, delegations, storage.signer())?;

    common::IdRef::from(urn).update(storage, next.content_id, "update")?;
    if let Some(local_id) = whoami.into() {
        local_id.link(storage, urn)?;
    }

    Ok(next)
}

/// Merge and sign the [`User`] state as seen by `from`.
#[tracing::instrument(level = "debug", skip(storage), err)]
pub fn merge<S>(storage: &Storage<S>, urn: &Urn, from: PeerId) -> Result<User, Error>
where
    S: Signer,
{
    let ours = get(storage, urn)?.ok_or_else(|| Error::NotFound(urn.clone()))?;
    let theirs = {
        let their_urn = Urn {
            id: urn.id,
            path: Some(reflike!("remotes").join(from).join(&*urn::DEFAULT_PATH)),
        };
        get(storage, &their_urn)?.ok_or_else(|| Error::NotFound(their_urn))?
    };

    let ours = Verifying::from(ours).signed()?;
    let theirs = Verifying::from(theirs).signed()?;
    let next = identities(storage).update_from(ours, theirs, storage.signer())?;

    common::IdRef::from(urn).update(storage, next.content_id, &format!("merge from {}", from))?;

    Ok(next)
}

fn identities<S>(storage: &Storage<S>) -> Identities<User>
where
    S: Signer,
{
    storage.identities()
}
