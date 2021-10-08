// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::{
    git::{identities, Urn},
    profile::Profile,
};
use rad_clib::{keys::ssh::SshAuthSock, storage::ssh};

use crate::{cli::args::local::*, local, person};

pub fn eval(profile: &Profile, sock: SshAuthSock, opts: Options) -> anyhow::Result<()> {
    match opts {
        Options::Set(Set { urn }) => eval_set(profile, sock, urn)?,
        Options::Get(Get { urn }) => eval_get(profile, sock, urn)?,
        Options::Default(Default {}) => eval_default(profile, sock)?,
    }

    Ok(())
}

fn eval_set(profile: &Profile, sock: SshAuthSock, urn: Urn) -> anyhow::Result<()> {
    let (_, storage) = ssh::storage(profile, sock)?;
    let identity = local::get(&storage, urn.clone())?
        .ok_or_else(|| identities::Error::NotFound(urn.clone()))?;
    local::set(&storage, identity)?;
    println!("set default identity to `{}`", urn);
    Ok(())
}

fn eval_get(profile: &Profile, sock: SshAuthSock, urn: Urn) -> anyhow::Result<()> {
    let (_, storage) = ssh::storage(profile, sock)?;
    let identity = local::get(&storage, urn.clone())?
        .ok_or_else(|| identities::Error::NotFound(urn.clone()))?;
    println!(
        "{}",
        serde_json::to_string(&person::Display::from(identity.into_inner()))?
    );
    Ok(())
}

fn eval_default(profile: &Profile, sock: SshAuthSock) -> anyhow::Result<()> {
    let (_, storage) = ssh::storage(profile, sock)?;
    let identity = local::default(&storage)?;
    println!(
        "{}",
        serde_json::to_string(&person::Display::from(identity.into_inner()))?
    );
    Ok(())
}
