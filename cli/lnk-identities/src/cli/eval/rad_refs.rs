// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::{
    git::{storage::ReadOnly, Urn},
    profile::Profile,
    PeerId,
};

use crate::{cli::args::rad_refs::*, rad_refs, NotFound};

pub fn eval(profile: &Profile, opts: Options) -> anyhow::Result<()> {
    match opts {
        Options::RadSelf(RadSelf { urn, peer }) => eval_rad_self(profile, urn, peer)?,
        Options::Signed(Signed { urn, peer }) => eval_signed(profile, urn, peer)?,
        Options::Delegates(Delegates { urn, peer }) => eval_delegates(profile, urn, peer)?,
        Options::Delegate(Delegate {
            urn,
            delegate,
            peer,
        }) => eval_delegate(profile, urn, delegate, peer)?,
    }

    Ok(())
}

fn eval_rad_self(profile: &Profile, urn: Urn, peer: Option<PeerId>) -> anyhow::Result<()> {
    let paths = profile.paths();
    let storage = ReadOnly::open(paths)?;
    let person = rad_refs::rad_self(&storage, &urn, peer)?.ok_or(NotFound { urn, peer })?;
    println!("{}", serde_json::to_string(person.payload())?);
    Ok(())
}

fn eval_signed(profile: &Profile, urn: Urn, peer: Option<PeerId>) -> anyhow::Result<()> {
    let paths = profile.paths();
    let storage = ReadOnly::open(paths)?;
    let refs = rad_refs::rad_signed(&storage, &urn, peer)?.ok_or(NotFound { urn, peer })?;
    println!("{}", serde_json::to_string(&refs)?);
    Ok(())
}

fn eval_delegates(profile: &Profile, urn: Urn, peer: Option<PeerId>) -> anyhow::Result<()> {
    let paths = profile.paths();
    let storage = ReadOnly::open(paths)?;
    let references = rad_refs::rad_delegates(&storage, &urn, peer)?;

    for reference in references {
        match reference {
            Err(err) => eprintln!("{}", err),
            Ok(person) => println!("{}", serde_json::to_string(person.payload())?),
        }
    }

    Ok(())
}

fn eval_delegate(
    profile: &Profile,
    urn: Urn,
    delegate: Urn,
    peer: Option<PeerId>,
) -> anyhow::Result<()> {
    let paths = profile.paths();
    let storage = ReadOnly::open(paths)?;
    let person =
        rad_refs::rad_delegate(&storage, &urn, &delegate, peer)?.ok_or(NotFound { urn, peer })?;
    println!("{}", serde_json::to_string(person.payload())?);
    Ok(())
}
