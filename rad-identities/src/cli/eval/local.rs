// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::{
    git::{identities, Urn},
    profile::Profile,
};
use rad_clib::storage::ssh;

use crate::{cli::args::local::*, local};

pub fn eval(profile: &Profile, opts: Options) -> anyhow::Result<()> {
    match opts {
        Options::Set(Set { urn }) => eval_set(profile, urn)?,
        Options::Get(Get { urn }) => eval_get(profile, urn)?,
        Options::Default(Default {}) => eval_default(profile)?,
    }

    Ok(())
}

fn eval_set(profile: &Profile, urn: Urn) -> anyhow::Result<()> {
    let (_, storage) = ssh::storage(profile)?;
    let identity = local::get(&storage, urn.clone())?
        .ok_or_else(|| identities::Error::NotFound(urn.clone()))?;
    local::set(&storage, identity)?;
    println!("set default identity to `{}`", urn);
    Ok(())
}

fn eval_get(profile: &Profile, urn: Urn) -> anyhow::Result<()> {
    let (_, storage) = ssh::storage(profile)?;
    let identity = local::get(&storage, urn.clone())?
        .ok_or_else(|| identities::Error::NotFound(urn.clone()))?;
    println!(
        "{}",
        serde_json::to_string(&identity.into_inner().payload())?
    );
    Ok(())
}

fn eval_default(profile: &Profile) -> anyhow::Result<()> {
    let (_, storage) = ssh::storage(profile)?;
    let identity = local::default(&storage)?;
    println!("{}", identity.urn());
    println!(
        "{}",
        serde_json::to_string(identity.into_inner().payload())?
    );
    Ok(())
}
