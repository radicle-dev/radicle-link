// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use thrussh_agent::{client::ClientStream, Constraint};

use rad_clib::keys;

use crate::{create, get, list, paths, peer_id, set, ssh_add};

use super::args::*;

pub fn main<S>(Args { command }: Args) -> anyhow::Result<()>
where
    S: ClientStream + Unpin + 'static,
{
    eval::<S>(command)
}

fn eval<S>(command: Command) -> anyhow::Result<()>
where
    S: ClientStream + Unpin + 'static,
{
    match command {
        Command::Create(Create {}) => {
            let (profile, peer_id) = create(keys::prompt::new())?;
            println!("profile id: {}", profile.id());
            println!("peer id: {}", peer_id);
        },
        Command::Get(Get { id }) => {
            let profile = get(id)?;
            match profile {
                Some(profile) => println!("{}", profile.id()),
                None => println!(
                    "no active profile found, perhaps you want to run `rad profile create`?"
                ),
            }
        },
        Command::Set(Set { id }) => {
            set(id.clone())?;
            println!("successfully set active profile id to {}", id);
        },
        Command::List(List {}) => {
            let profiles = list()?;
            for profile in profiles {
                println!("{}", profile.id());
            }
        },
        Command::Peer(GetPeerId { id }) => {
            let peer_id = peer_id(id)?;
            println!("{}", peer_id);
        },
        Command::Paths(GetPaths { id }) => {
            let paths = paths(id)?;
            println!("git: {}", paths.git_dir().display());
            println!("git includes: {}", paths.git_includes_dir().display());
            println!("keys: {}", paths.keys_dir().display());
        },
        Command::SshAdd(SshAdd { id, time }) => {
            let constraint = time.map_or(Constraint::Confirm, |seconds| Constraint::KeyLifetime {
                seconds,
            });
            let id = ssh_add::<S, _, _>(id, keys::prompt::new(), &[constraint])?;
            println!("added key for profile id `{}`", id);
        },
    }

    Ok(())
}
