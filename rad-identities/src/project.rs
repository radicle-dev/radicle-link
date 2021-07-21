// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::BTreeSet, convert::TryFrom as _, fmt, path::PathBuf};

use either::Either;
use thiserror::Error;

use librad::{
    crypto::BoxedSigner,
    git::{
        identities::{self, local::LocalIdentity, project, Project},
        local::{transport, url::LocalUrl},
        storage::{ReadOnly, Storage},
        types::{Namespace, Reference},
        Urn,
    },
    identities::{
        delegation::{indirect, Indirect},
        git::Revision,
        payload::{KeyOrUrn, ProjectPayload},
        IndirectDelegation,
    },
    paths::Paths,
    PeerId,
};

use crate::{
    git::{self, checkout, include},
    MissingDefaultIdentity,
};

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Checkout(#[from] checkout::Error),

    #[error(transparent)]
    Identities(Box<identities::Error>),

    #[error(transparent)]
    Include(#[from] Box<include::Error>),

    #[error(transparent)]
    Indirect(#[from] indirect::error::FromIter<Revision>),

    #[error(transparent)]
    Local(#[from] identities::local::Error),

    #[error(transparent)]
    MissingDefault(#[from] MissingDefaultIdentity),
}

impl From<identities::Error> for Error {
    fn from(err: identities::Error) -> Self {
        Self::Identities(Box::new(err))
    }
}

impl From<include::Error> for Error {
    fn from(err: include::Error) -> Self {
        Self::Include(Box::new(err))
    }
}

pub enum Creation {
    New { path: Option<PathBuf> },
    Existing { path: PathBuf },
}

pub enum WhoAmI {
    Default,
    Urn(Urn),
}

impl From<Option<Urn>> for WhoAmI {
    fn from(urn: Option<Urn>) -> Self {
        urn.map_or(Self::Default, Self::Urn)
    }
}

impl WhoAmI {
    fn resolve(self, storage: &Storage) -> Result<LocalIdentity, Error> {
        Ok(match self {
            Self::Default => identities::local::default(storage)?.ok_or(MissingDefaultIdentity)?,
            Self::Urn(urn) => identities::local::load(storage, urn.clone())?
                .ok_or(identities::Error::NotFound(urn))?,
        })
    }
}

pub fn create<P>(
    storage: &Storage,
    paths: Paths,
    signer: BoxedSigner,
    whoami: WhoAmI,
    payload: P,
    creation: Creation,
) -> anyhow::Result<Project>
where
    P: Into<ProjectPayload> + fmt::Debug,
{
    let payload = payload.into();
    let whoami = whoami.resolve(storage)?;
    let delegations = Indirect::from(whoami.clone().into_inner().into_inner());

    let urn = project::urn(storage, payload.clone(), delegations.clone())?;
    let url = LocalUrl::from(urn);
    let settings = transport::Settings {
        paths: paths.clone(),
        signer,
    };

    match creation {
        Creation::New { path } => {
            if let Some(path) = path {
                let valid = git::new::New::new(payload.clone(), path).validate()?;
                valid.init(url, settings)?;
            }
        },
        Creation::Existing { path } => {
            let valid = git::existing::Existing::new(payload.clone(), path).validate()?;
            valid.init(url, settings)?;
        },
    }

    let project = project::create(storage, whoami, payload, delegations)?;
    include::update(storage, &paths, &project)?;

    Ok(project)
}

pub fn get<S>(storage: &S, urn: &Urn) -> Result<Option<Project>, Error>
where
    S: AsRef<ReadOnly>,
{
    Ok(project::get(storage, urn)?)
}

pub fn list<S>(
    storage: &S,
) -> Result<impl Iterator<Item = Result<Project, identities::Error>> + '_, Error>
where
    S: AsRef<ReadOnly>,
{
    Ok(crate::any::list(storage, |i| i.project())?)
}

pub fn update<P>(
    storage: &Storage,
    urn: &Urn,
    whoami: Option<Urn>,
    payload: P,
    delegations: BTreeSet<KeyOrUrn<Revision>>,
) -> Result<Project, Error>
where
    P: Into<Option<ProjectPayload>> + fmt::Debug,
{
    let whoami = whoami
        .and_then(|me| identities::local::load(storage, me).transpose())
        .transpose()?;

    let delegations = if delegations.is_empty() {
        None
    } else {
        Some(resolve_indirect(storage, delegations)?)
    };
    Ok(project::update(storage, urn, whoami, payload, delegations)?)
}

fn resolve_indirect(
    storage: &Storage,
    delegations: BTreeSet<KeyOrUrn<Revision>>,
) -> Result<IndirectDelegation, Error> {
    Ok(Indirect::try_from_iter(
        delegations
            .into_iter()
            .map(|kou| match kou.into() {
                Either::Left(key) => Ok(Either::Left(key)),
                Either::Right(urn) => identities::person::verify(storage, &urn)
                    .and_then(|person| person.ok_or(identities::Error::NotFound(urn)))
                    .map(|person| Either::Right(person.into_inner())),
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter(),
    )?)
}

pub fn checkout<S>(
    storage: &S,
    paths: Paths,
    signer: BoxedSigner,
    urn: &Urn,
    peer: Option<PeerId>,
    path: PathBuf,
) -> Result<git2::Repository, Error>
where
    S: AsRef<ReadOnly>,
{
    let local = storage.as_ref().peer_id();
    let project = get(storage, urn)?.ok_or_else(|| identities::Error::NotFound(urn.clone()))?;
    let remote = peer
        .and_then(|peer| {
            if peer == *local {
                None
            } else {
                let urn = Urn::try_from(Reference::rad_self(Namespace::from(&project.urn()), peer))
                    .expect("namespace is set");
                Some(identities::person::get(&storage, &urn).and_then(|person| {
                    person
                        .ok_or_else(|| identities::Error::NotFound(urn.clone()))
                        .map(|person| (person, peer))
                }))
            }
        })
        .transpose()?;
    let from = checkout::from_whom(&project, remote, path)?;
    let settings = transport::Settings {
        paths: paths.clone(),
        signer,
    };
    let repo = git::checkout::checkout(settings, &project, from)?;
    include::update(&storage, &paths, &project)?;
    Ok(repo)
}

pub fn review() {
    todo!()
}
