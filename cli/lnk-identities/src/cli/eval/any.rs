// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::{
    git::{identities, storage::ReadOnly, Urn},
    profile::Profile,
};

use crate::{any, cli::args::any::*};

pub fn eval(profile: &Profile, opts: Options) -> anyhow::Result<()> {
    match opts {
        Options::Get(Get { urn }) => eval_get(profile, urn)?,
        Options::List(List {}) => eval_list(profile)?,
    }

    Ok(())
}

fn eval_get(profile: &Profile, urn: Urn) -> anyhow::Result<()> {
    let paths = profile.paths();
    let storage = ReadOnly::open(paths)?;
    let identity =
        any::get(&storage, &urn)?.ok_or_else(|| identities::Error::NotFound(urn.clone()))?;
    println!("{}", serde_json::to_string(&any::Display::from(identity))?);
    Ok(())
}

fn eval_list(profile: &Profile) -> anyhow::Result<()> {
    let paths = profile.paths();
    let storage = ReadOnly::open(paths)?;
    let identities = any::list(&storage, Some)?;
    let identities = identities
        .map(|p| p.map(any::Display::from))
        .collect::<Result<Vec<_>, _>>()?;
    println!("{}", serde_json::to_string(&identities)?);
    Ok(())
}
