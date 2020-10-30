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

use either::Either;
use git_ext::is_not_found_err;

use super::{
    super::{
        storage2::{self, Storage},
        types::{namespace, Force, Reference, Single, SymbolicRef},
    },
    common,
    error::Error,
    local::LocalIdentity,
};
use crate::{
    identities::{
        self,
        git::{Identities, IndirectDelegation, Project, Revision, VerifiedProject, Verifying},
        urn,
    },
    peer::PeerId,
    signer::Signer,
};

pub use identities::{git::Urn, payload::ProjectPayload};

type Namespace = namespace::Namespace<Revision>;

/// Read a [`Project`] from the tip of the ref [`Urn::path`] points to.
///
/// If the ref is not found, `None` is returned.
#[tracing::instrument(level = "trace", skip(storage), err)]
pub fn get<S>(storage: &Storage<S>, urn: &Urn) -> Result<Option<Project>, Error>
where
    S: Signer,
{
    match storage.reference(&Reference::try_from(urn)?) {
        Ok(Some(reference)) => {
            let tip = reference.peel_to_commit()?.id();
            Ok(Some(identities(storage).get(tip)?))
        },

        Ok(None) => Ok(None),
        Err(storage2::Error::Git(e)) if is_not_found_err(&e) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Read and verify the [`Project`] pointed to by `urn`.
///
/// If the ref pointed to by [`Urn::path`] is not found, `None` is returned.
///
/// # Caveats
///
/// Keep in mind that the `content_id` of a successfully verified project may
/// not be the same as the tip of the ref [`Urn::path`] points to. That is, this
/// function cannot be used to assert that the state after an [`update`] is
/// valid.
#[tracing::instrument(level = "debug", skip(storage), err)]
pub fn verify<S>(storage: &Storage<S>, urn: &Urn) -> Result<Option<VerifiedProject>, Error>
where
    S: Signer,
{
    match storage.reference(&Reference::try_from(urn)?) {
        Ok(Some(reference)) => {
            let tip = reference.peel_to_commit()?.id();
            let lookup = |urn| {
                let refname = Reference::<_, PeerId, _>::rad_id(Namespace::from(urn)).to_string();
                storage.as_raw().refname_to_id(&refname)
            };
            identities(storage)
                .verify(tip, lookup)
                .map(Some)
                .map_err(|e| Error::Verify(e.into()))
        },

        Ok(None) => Ok(None),
        Err(storage2::Error::Git(e)) if is_not_found_err(&e) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Create a new [`Project`].
#[tracing::instrument(level = "debug", skip(storage, whoami), err)]
pub fn create<S, P>(
    storage: &Storage<S>,
    whoami: LocalIdentity,
    payload: P,
    delegations: IndirectDelegation,
) -> Result<Project, Error>
where
    S: Signer,
    P: Into<ProjectPayload> + Debug,
{
    let project = identities(storage).create(payload.into(), delegations, storage.signer())?;
    Refs::Create(&project).apply(storage)?;
    whoami.link(storage, &project.urn())?;

    Ok(project)
}

/// Update the [`Project`] at `urn`.
#[tracing::instrument(level = "debug", skip(storage), err)]
pub fn update<S, L, P, D>(
    storage: &Storage<S>,
    urn: &Urn,
    whoami: L,
    payload: P,
    delegations: D,
) -> Result<Project, Error>
where
    S: Signer,
    L: Into<Option<LocalIdentity>> + Debug,
    P: Into<Option<ProjectPayload>> + Debug,
    D: Into<Option<IndirectDelegation>> + Debug,
{
    let prev = get(storage, urn)?.ok_or_else(|| Error::NotFound(urn.clone()))?;
    let prev = Verifying::from(prev).signed()?;
    let next = identities(storage).update(prev, payload, delegations, storage.signer())?;

    Refs::Update(&next, "update").apply(storage)?;
    if let Some(local_id) = whoami.into() {
        local_id.link(storage, urn)?;
    }

    Ok(next)
}

/// Merge and sign the [`Project`] state as seen by `from`.
#[tracing::instrument(level = "debug", skip(storage), err)]
pub fn merge<S>(storage: &Storage<S>, urn: &Urn, from: PeerId) -> Result<Project, Error>
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

    Refs::Update(&next, &format!("merge from {}", from)).apply(storage)?;

    Ok(next)
}

enum Refs<'a> {
    Create(&'a Project),
    Update(&'a Project, &'a str),
}

impl<'a> Refs<'a> {
    pub fn apply<S>(&self, storage: &Storage<S>) -> Result<(), Error>
    where
        S: Signer,
    {
        for symref in self.delegates() {
            symref.create(storage.as_raw())?;
        }
        match self {
            Self::Create(project) => {
                common::IdRef::from(&project.urn()).create(storage, project.content_id)
            },
            Self::Update(project, msg) => {
                common::IdRef::from(&project.urn()).update(storage, project.content_id, msg)
            },
        }?;

        Ok(())
    }

    fn project(&self) -> &Project {
        match self {
            Self::Create(project) => project,
            Self::Update(project, _) => project,
        }
    }

    fn delegates(
        &'a self,
    ) -> impl Iterator<
        Item = SymbolicRef<
            Reference<Namespace, PeerId, Single>,
            Reference<Namespace, PeerId, Single>,
        >,
    > + 'a {
        let source = self.project().urn();
        (&self.project().doc.delegations)
            .into_iter()
            .filter_map(Either::right)
            .map(move |id| {
                let urn = id.urn();
                SymbolicRef {
                    source: Reference::rad_delegate(Namespace::from(&source), &urn),
                    target: Reference::rad_id(Namespace::from(&urn)),
                    force: Force::True,
                }
            })
    }
}

fn identities<S>(storage: &Storage<S>) -> Identities<Project>
where
    S: Signer,
{
    storage.identities()
}
