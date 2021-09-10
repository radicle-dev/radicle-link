// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use thrussh_agent::client::ClientStream;

use librad::profile::{Profile, ProfileId, RadHome};

use super::{
    args::{Args, Command},
    eval::{any, local, person, project, rad_refs, refs, tracking},
};

pub fn main<S>(Args { command }: Args, profile: Option<ProfileId>) -> anyhow::Result<()>
where
    S: ClientStream + Unpin + 'static,
{
    let home = RadHome::default();
    let profile = Profile::from_home(&home, profile)?;

    match command {
        Command::Project(opts) => project::eval::<S>(&profile, opts.project)?,
        Command::Person(opts) => person::eval::<S>(&profile, opts.person)?,
        Command::Any(opts) => any::eval(&profile, opts.any)?,
        Command::Local(opts) => local::eval::<S>(&profile, opts.local)?,
        Command::RadRefs(opts) => rad_refs::eval(&profile, opts.rad_refs)?,
        Command::Refs(opts) => refs::eval(&profile, opts.refs)?,
        Command::Track(track) => tracking::eval_track::<S>(&profile, track)?,
        Command::Untrack(untrack) => tracking::eval_untrack::<S>(&profile, untrack)?,
    }

    Ok(())
}