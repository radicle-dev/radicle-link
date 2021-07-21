// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use thrussh_agent::client::ClientStream;

use librad::{
    git::{identities, Urn},
    profile::Profile,
};
use rad_clib::storage::ssh;

use crate::{cli::args::local::*, local};

pub async fn eval<S>(profile: &Profile, opts: Options) -> anyhow::Result<()>
where
    S: ClientStream + Unpin + 'static,
{
    match opts {
        Options::Set(Set { urn }) => eval_set::<S>(profile, urn).await?,
        Options::Get(Get { urn }) => eval_get::<S>(profile, urn).await?,
        Options::Default(Default {}) => eval_default::<S>(profile).await?,
    }

    Ok(())
}

async fn eval_set<S>(profile: &Profile, urn: Urn) -> anyhow::Result<()>
where
    S: ClientStream + Unpin + 'static,
{
    let (_, storage) = ssh::storage::<S>(profile).await?;
    let identity = local::get(&storage, urn.clone())?
        .ok_or_else(|| identities::Error::NotFound(urn.clone()))?;
    local::set(&storage, identity)?;
    println!("set default identity to `{}`", urn);
    Ok(())
}

async fn eval_get<S>(profile: &Profile, urn: Urn) -> anyhow::Result<()>
where
    S: ClientStream + Unpin + 'static,
{
    let (_, storage) = ssh::storage::<S>(profile).await?;
    let identity = local::get(&storage, urn.clone())?
        .ok_or_else(|| identities::Error::NotFound(urn.clone()))?;
    println!(
        "{}",
        serde_json::to_string(&identity.into_inner().payload())?
    );
    Ok(())
}

async fn eval_default<S>(profile: &Profile) -> anyhow::Result<()>
where
    S: ClientStream + Unpin + 'static,
{
    let (_, storage) = ssh::storage::<S>(profile).await?;
    let identity = local::default(&storage)?;
    println!("{}", identity.urn());
    println!(
        "{}",
        serde_json::to_string(identity.into_inner().payload())?
    );
    Ok(())
}
