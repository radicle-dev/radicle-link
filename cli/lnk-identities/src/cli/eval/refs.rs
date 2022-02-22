// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryFrom as _;

use librad::{
    git::{storage::ReadOnly, Urn},
    git_ext::RefLike,
    profile::Profile,
    PeerId,
};

use crate::{cli::args::refs::*, refs, NotFound};

pub fn eval(profile: &Profile, opts: Options) -> anyhow::Result<()> {
    match opts {
        Options::Heads(Heads { urn, peer }) => eval_heads(profile, urn, peer)?,
        Options::Tags(Tags { urn, peer }) => eval_tags(profile, urn, peer)?,
        Options::Notes(Notes { urn, peer }) => eval_notes(profile, urn, peer)?,
        Options::Category(Category {
            urn,
            peer,
            category,
        }) => eval_category(profile, urn, peer, category)?,
    }

    Ok(())
}

fn eval_heads(profile: &Profile, urn: Urn, peer: Option<PeerId>) -> anyhow::Result<()> {
    let paths = profile.paths();
    let storage = ReadOnly::open(paths)?;
    let heads = refs::heads(&storage, &urn, peer)?.ok_or(NotFound { urn, peer })?;
    println!("{}", serde_json::to_string(&heads)?);
    Ok(())
}

fn eval_tags(profile: &Profile, urn: Urn, peer: Option<PeerId>) -> anyhow::Result<()> {
    let paths = profile.paths();
    let storage = ReadOnly::open(paths)?;
    let tags = refs::tags(&storage, &urn, peer)?.ok_or(NotFound { urn, peer })?;
    println!("{}", serde_json::to_string(&tags)?);
    Ok(())
}

fn eval_notes(profile: &Profile, urn: Urn, peer: Option<PeerId>) -> anyhow::Result<()> {
    let paths = profile.paths();
    let storage = ReadOnly::open(paths)?;
    let notes = refs::notes(&storage, &urn, peer)?.ok_or(NotFound { urn, peer })?;
    println!("{}", serde_json::to_string(&notes)?);
    Ok(())
}

fn eval_category(
    profile: &Profile,
    urn: Urn,
    peer: Option<PeerId>,
    category: String,
) -> anyhow::Result<()> {
    let paths = profile.paths();
    let storage = ReadOnly::open(paths)?;
    let category = RefLike::try_from(category)?;
    let refs = refs::category(&storage, &urn, peer, category)?;
    println!("{}", serde_json::to_string(&refs)?);
    Ok(())
}
