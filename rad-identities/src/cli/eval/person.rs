// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom as _, path::PathBuf};

use anyhow::anyhow;
use thrussh_agent::client::ClientStream;

use librad::{
    crypto::PublicKey,
    git::{
        identities,
        types::{Namespace, Reference},
        Urn,
    },
    identities::payload::PersonPayload,
    profile::Profile,
    PeerId,
};
use rad_clib::storage::{self, ssh};

use crate::{cli::args::person::*, person};

pub async fn eval<S>(profile: &Profile, opts: Options) -> anyhow::Result<()>
where
    S: ClientStream + Unpin + 'static,
{
    match opts {
        Options::Create(CreateOptions { create }) => eval_create::<S>(profile, create).await?,
        Options::Get(Get { urn, peer }) => eval_get(profile, urn, peer)?,
        Options::List(List {}) => eval_list(profile)?,
        Options::Update(Update {
            urn,
            whoami,
            payload,
            delegations,
        }) => eval_update::<S>(profile, urn, whoami, payload, delegations).await?,
        Options::Checkout(Checkout { urn, path, peer }) => {
            eval_checkout::<S>(profile, urn, path, peer).await?
        },
        Options::Review(Review {}) => unimplemented!(),
    }

    Ok(())
}

async fn eval_create<S>(profile: &Profile, create: Create) -> anyhow::Result<()>
where
    S: ClientStream + Unpin + 'static,
{
    let (signer, storage) = ssh::storage::<S>(profile).await?;
    let paths = profile.paths();
    let person = match create {
        Create::New(New { payload, path }) => person::create(
            &storage,
            paths.clone(),
            signer,
            payload,
            person::Creation::New { path },
        )?,
        Create::Existing(Existing { payload, path }) => person::create(
            &storage,
            paths.clone(),
            signer,
            payload,
            person::Creation::Existing { path },
        )?,
    };
    println!("{}", serde_json::to_string(&person.subject())?);
    Ok(())
}

fn eval_get(profile: &Profile, urn: Urn, peer: Option<PeerId>) -> anyhow::Result<()> {
    let storage = storage::read_only(profile)?;
    let rad = Reference::rad_id(Namespace::from(&urn)).with_remote(peer);
    let urn = Urn::try_from(rad).map_err(|err| anyhow!(err))?;
    let project =
        person::get(&storage, &urn)?.ok_or_else(|| identities::Error::NotFound(urn.clone()))?;
    println!("{}", serde_json::to_string(&project.payload())?);
    Ok(())
}

fn eval_list(profile: &Profile) -> anyhow::Result<()> {
    let storage = storage::read_only(profile)?;
    let persons = person::list(&storage)?;
    let persons = persons
        .map(|p| p.map(|p| p.payload().clone()))
        .collect::<Result<Vec<_>, _>>()?;
    println!("{}", serde_json::to_string(&persons)?);
    Ok(())
}

async fn eval_update<S>(
    profile: &Profile,
    urn: Urn,
    whoami: Option<Urn>,
    payload: Option<PersonPayload>,
    delegations: Vec<PublicKey>,
) -> anyhow::Result<()>
where
    S: ClientStream + Unpin + 'static,
{
    let (_, storage) = ssh::storage::<S>(profile).await?;
    let person = person::update(&storage, &urn, whoami, payload, delegations.into_iter())?;
    println!("{}", serde_json::to_string(person.payload())?);
    Ok(())
}

async fn eval_checkout<S>(
    profile: &Profile,
    urn: Urn,
    path: PathBuf,
    peer: Option<PeerId>,
) -> anyhow::Result<()>
where
    S: ClientStream + Unpin + 'static,
{
    let paths = profile.paths();
    let (signer, storage) = ssh::storage::<S>(profile).await?;
    let repo = person::checkout(&storage, paths.clone(), signer, &urn, peer, path)?;
    println!("working copy created at `{}`", repo.path().display());
    Ok(())
}
