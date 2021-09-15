// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom as _, path::PathBuf};

use thiserror::Error;

use librad::{
    crypto::{BoxedSigner, PublicKey},
    git::{
        identities::{self, person, Person},
        local::{transport, url::LocalUrl},
        storage::{ReadOnly, Storage},
        types::{Namespace, Reference},
        Urn,
    },
    identities::{
        delegation::Direct,
        payload::{self, PersonPayload},
    },
    paths::Paths,
    PeerId,
};

use crate::git::{self, checkout, include};

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Checkout(#[from] checkout::Error),

    #[error(transparent)]
    Ext(#[from] payload::ExtError),

    #[error(transparent)]
    Identities(#[from] identities::Error),

    #[error(transparent)]
    Include(#[from] include::Error),

    #[error(transparent)]
    Local(#[from] identities::local::Error),
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct PersonDisplay {
    pub urn: Urn,
    pub payload: PersonPayload,
}

pub enum Creation {
    New { path: Option<PathBuf> },
    Existing { path: PathBuf },
}

pub fn create<T>(
    storage: &Storage,
    paths: Paths,
    signer: BoxedSigner,
    payload: payload::Person,
    ext: Vec<payload::Ext<T>>,
    mut delegations: Vec<PublicKey>,
    creation: Creation,
) -> anyhow::Result<Person>
where
    T: serde::Serialize,
{
    let mut payload = PersonPayload::new(payload);
    for e in ext.into_iter() {
        payload.set_ext(e)?;
    }

    let key = *storage.peer_id().as_public_key();
    delegations.push(key);
    let delegations: Direct = delegations.into_iter().collect();

    let urn = person::urn(storage, payload.clone(), delegations.clone())?;
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
            };
        },
        Creation::Existing { path } => {
            let valid = git::existing::Existing::new(payload.clone(), path).validate()?;
            valid.init(url, settings)?;
        },
    }

    let person = person::create(storage, payload, delegations)?;
    include::update(&storage, &paths, &person)?;

    Ok(person)
}

pub fn get<S>(storage: &S, urn: &Urn) -> Result<Option<Person>, Error>
where
    S: AsRef<ReadOnly>,
{
    Ok(person::get(storage, urn)?)
}

pub fn list<S>(
    storage: &S,
) -> Result<impl Iterator<Item = Result<Person, identities::Error>> + '_, Error>
where
    S: AsRef<ReadOnly>,
{
    Ok(crate::any::list(storage, |i| i.person())?)
}

pub fn update(
    storage: &Storage,
    urn: &Urn,
    whoami: Option<Urn>,
    payload: Option<payload::Person>,
    mut ext: Vec<payload::Ext<serde_json::Value>>,
    delegations: impl Iterator<Item = PublicKey>,
) -> Result<Person, Error> {
    let old =
        person::verify(storage, urn)?.ok_or_else(|| identities::Error::NotFound(urn.clone()))?;
    let mut old_payload = old.payload().clone();
    let payload = match payload {
        None => {
            for e in ext {
                old_payload.set_ext(e)?;
            }
            old_payload
        },
        Some(payload) => {
            let mut payload = payload::PersonPayload::new(payload);
            ext.extend(old_payload.exts().map(|(url, val)| payload::Ext {
                namespace: url.clone(),
                val: val.clone(),
            }));
            for e in ext {
                payload.set_ext(e)?;
            }
            payload
        },
    };
    let whoami = whoami
        .and_then(|me| identities::local::load(storage, me).transpose())
        .transpose()?;
    Ok(person::update(
        storage,
        urn,
        whoami,
        Some(payload),
        Some(delegations.collect()),
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
    let person = get(storage, urn)?.ok_or_else(|| identities::Error::NotFound(urn.clone()))?;
    let remote = peer
        .and_then(|peer| {
            if peer == *local {
                None
            } else {
                let urn = Urn::try_from(Reference::rad_self(Namespace::from(&person.urn()), peer))
                    .expect("namespace is set");
                Some(identities::person::get(&storage, &urn).and_then(|person| {
                    person
                        .ok_or_else(|| identities::Error::NotFound(urn.clone()))
                        .map(|person| (person, peer))
                }))
            }
        })
        .transpose()?;
    let from = checkout::from_whom(&person, remote, path)?;
    let settings = transport::Settings {
        paths: paths.clone(),
        signer,
    };
    let repo = git::checkout::checkout(settings, &person, from)?;
    include::update(&storage, &paths, &person)?;
    Ok(repo)
}

pub fn review() {
    todo!()
}
