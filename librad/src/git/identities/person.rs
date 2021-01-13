// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

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
};

pub use identities::{
    git::{Person, Urn, VerifiedPerson},
    payload::PersonPayload,
};

/// Read a [`Person`] from the tip of the ref [`Urn::path`] points to.
///
/// If the ref is not found, `None` is returned.
#[tracing::instrument(level = "trace", skip(storage), err)]
pub fn get(storage: &Storage, urn: &Urn) -> Result<Option<Person>, Error> {
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

/// Read and verify the [`Person`] pointed to by `urn`.
///
/// If the ref pointed to by [`Urn::path`] is not found, `None` is returned.
///
/// # Caveats
///
/// Keep in mind that the `content_id` of a successfully verified person may
/// not be the same as the tip of the ref [`Urn::path`] points to. That is, this
/// function cannot be used to assert that the state after an [`update`] is
/// valid.
#[tracing::instrument(level = "debug", skip(storage), err)]
pub fn verify(storage: &Storage, urn: &Urn) -> Result<Option<VerifiedPerson>, Error> {
    let branch = Reference::try_from(urn)?;
    tracing::debug!("verifying {} from {}", urn, branch);
    match storage.reference(&branch) {
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

/// Create a new [`Person`].
///
/// The `delegations` must include the [`Storage`]'s [`crate::signer::Signer`]
/// key, such that the newly created [`Person`] is also a valid
/// [`LocalIdentity`] -- it is, in fact, its own [`LocalIdentity`]. This can be
/// changed via [`update`].
#[tracing::instrument(level = "debug", skip(storage), err)]
pub fn create<P>(
    storage: &Storage,
    payload: P,
    delegations: delegation::Direct,
) -> Result<Person, Error>
where
    P: Into<PersonPayload> + Debug,
{
    let person = {
        let person = identities(storage).create(payload.into(), delegations, storage.signer())?;
        let verified = identities(storage)
            .verify(*person.content_id)
            .map_err(|e| Error::Verify(e.into()))?;
        LocalIdentity::valid(verified, storage.signer())
    }?;

    let urn = person.urn();
    common::IdRef::from(&urn).create(storage, person.content_id)?;
    person.link(storage, &urn)?;

    Ok(person.into_inner().into_inner())
}

/// Update the [`Person`] at `urn`.
#[tracing::instrument(level = "debug", skip(storage), err)]
pub fn update<L, P, D>(
    storage: &Storage,
    urn: &Urn,
    whoami: L,
    payload: P,
    delegations: D,
) -> Result<Person, Error>
where
    L: Into<Option<LocalIdentity>> + Debug,
    P: Into<Option<PersonPayload>> + Debug,
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

/// Merge and sign the [`Person`] state as seen by `from`.
#[tracing::instrument(level = "debug", skip(storage), err)]
pub fn merge(storage: &Storage, urn: &Urn, from: PeerId) -> Result<Person, Error> {
    let ours = get(storage, urn)?.ok_or_else(|| Error::NotFound(urn.clone()))?;
    let theirs = {
        let rad_id = urn::DEFAULT_PATH.strip_prefix("refs").unwrap();
        let their_urn = Urn {
            id: urn.id,
            path: Some(reflike!("remotes").join(from).join(rad_id)),
        };
        get(storage, &their_urn)?.ok_or(Error::NotFound(their_urn))?
    };

    let ours = Verifying::from(ours).signed()?;
    let theirs = Verifying::from(theirs).signed()?;
    let next = identities(storage).update_from(ours, theirs, storage.signer())?;

    common::IdRef::from(urn).update(storage, next.content_id, &format!("merge from {}", from))?;

    Ok(next)
}

fn identities(storage: &Storage) -> Identities<Person> {
    storage.identities()
}
