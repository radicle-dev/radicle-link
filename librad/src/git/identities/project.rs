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

use std::{
    convert::{TryFrom, TryInto},
    path::Path,
};

use either::Either;
use thiserror::Error;

use super::common;
use crate::{
    git::{
        ext::is_not_found_err,
        storage2::{self, Storage},
        types::{reference, Force, Namespace2, Reference, SymbolicRef2},
    },
    identities::{
        self,
        git::{
            Identities,
            IndirectDelegation,
            Project,
            VerificationError,
            VerifiedProject,
            Verifying,
        },
        urn,
    },
    peer::PeerId,
    signer::Signer,
};

pub use identities::{git::Urn, payload::ProjectPayload};

#[derive(Debug, Error)]
pub enum Error<S: std::error::Error + Send + Sync + 'static> {
    #[error("the URN {0} already exists")]
    AlreadyExists(Urn),

    #[error("the URN {0} does not exist")]
    NotFound(Urn),

    #[error("malformed URN")]
    Ref(#[from] reference::FromUrnError),

    #[error(transparent)]
    Verify(#[from] identities::error::VerifyProject<git2::Error>),

    #[error(transparent)]
    Verification(#[from] VerificationError),

    #[error(transparent)]
    Config(#[from] storage2::config::Error),

    #[error(transparent)]
    Storage(#[from] storage2::Error),

    #[error(transparent)]
    Merge(#[from] identities::git::error::Merge<S>),

    #[error(transparent)]
    Load(#[from] identities::git::error::Load),

    #[error(transparent)]
    Store(#[from] identities::git::error::Store<S>),

    #[error(transparent)]
    Git(#[from] git2::Error),
}

pub fn get<S>(storage: &Storage<S>, urn: &Urn) -> Result<Option<Project>, Error<S::Error>>
where
    S: Signer,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    match storage.reference(&Reference::try_from(urn)?) {
        Ok(reference) => {
            let tip = reference.peel_to_commit()?.id();
            Ok(Some(identities(storage).get(tip)?))
        },

        Err(storage2::Error::Git(e)) if is_not_found_err(&e) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn verify<S>(
    storage: &Storage<S>,
    urn: &Urn,
) -> Result<Option<VerifiedProject>, Error<S::Error>>
where
    S: Signer,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    match storage.reference(&Reference::try_from(urn)?) {
        Ok(reference) => {
            let tip = reference.peel_to_commit()?.id();
            let lookup = |urn| {
                let refname = Reference::<_, PeerId, _>::rad_id(Namespace2::from(urn)).to_string();
                storage.as_raw().refname_to_id(&refname)
            };
            Ok(Some(identities(storage).verify(tip, lookup)?))
        },

        Err(storage2::Error::Git(e)) if is_not_found_err(&e) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn create<S>(
    storage: &Storage<S>,
    payload: impl Into<ProjectPayload>,
    delegations: IndirectDelegation,
) -> Result<Project, Error<S::Error>>
where
    S: Signer,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let project = identities(storage).create(payload.into(), delegations, storage.signer())?;

    Refs::Create(&project).apply(storage)?;

    Ok(project)
}

pub fn update<S>(
    storage: &Storage<S>,
    urn: &Urn,
    payload: impl Into<Option<ProjectPayload>>,
    delegations: impl Into<Option<IndirectDelegation>>,
) -> Result<Project, Error<S::Error>>
where
    S: Signer,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let delegations = delegations.into();

    let prev = get(storage, urn)?.ok_or_else(|| Error::NotFound(urn.clone()))?;
    let prev = Verifying::from(prev).signed()?;
    let next = identities(storage).update(prev, payload, delegations, storage.signer())?;

    Refs::Update(&next, "update").apply(storage)?;

    Ok(next)
}

pub fn merge<S>(storage: &Storage<S>, urn: &Urn, from: PeerId) -> Result<Project, Error<S::Error>>
where
    S: Signer,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let ours = get(storage, urn)?.ok_or_else(|| Error::NotFound(urn.clone()))?;
    let theirs = {
        let their_urn = Urn {
            id: urn.id,
            path: Some(
                Path::new("remotes")
                    .join(from.to_string())
                    .join(&*urn::DEFAULT_PATH)
                    .try_into()
                    .unwrap(),
            ),
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
    pub fn apply<S>(&self, storage: &Storage<S>) -> Result<(), Error<S::Error>>
    where
        S: Signer,
        S::Error: std::error::Error + Send + Sync + 'static,
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

    fn delegates(&'a self) -> impl Iterator<Item = SymbolicRef2<PeerId>> + 'a {
        let source = self.project().urn();
        (&self.project().doc.delegations)
            .into_iter()
            .filter_map(Either::right)
            .map(move |id| {
                let urn = id.urn();
                let symref: SymbolicRef2<PeerId> = SymbolicRef2 {
                    source: Reference::rad_delegate(Namespace2::from(&source).into(), &urn),
                    target: Reference::rad_id(Namespace2::from(&urn).into()),
                    force: Force::True,
                };

                symref
            })
    }
}

fn identities<S>(storage: &Storage<S>) -> Identities<Project>
where
    S: Signer,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    storage.identities()
}
