// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use thrussh_agent::Constraint;

use rad_clib::keys;

use crate::{create, get, list, paths, peer_id, set, ssh_add, ssh_remove, ssh_test};

use super::args::*;

pub fn main(Args { command }: Args) -> anyhow::Result<()> {
    eval(command)
}

fn eval(command: Command) -> anyhow::Result<()> {
    match command {
        Command::Create(Create {}) => {
            let (profile, peer_id) = create(None, keys::prompt::new())?;
            println!("profile id: {}", profile.id());
            println!("peer id: {}", peer_id);
        },
        Command::Get(Get { id }) => {
            let profile = get(None, id)?;
            match profile {
                Some(profile) => println!("{}", profile.id()),
                None => println!(
                    "no active profile found, perhaps you want to run `rad profile create`?"
                ),
            }
        },
        Command::Set(Set { id }) => {
            set(None, id.clone())?;
            println!("successfully set active profile id to {}", id);
        },
        Command::List(List {}) => {
            let profiles = list(None)?;
            for profile in profiles {
                println!("{}", profile.id());
            }
        },
        Command::Peer(GetPeerId { id }) => {
            let peer_id = peer_id(None, id)?;
            println!("{}", peer_id);
        },
        Command::Paths(GetPaths { id }) => {
            let paths = paths(None, id)?;
            println!("git: {}", paths.git_dir().display());
            println!("git includes: {}", paths.git_includes_dir().display());
            println!("keys: {}", paths.keys_dir().display());
        },
        Command::Ssh(Ssh { options }) => match options {
            ssh::Options::Add(ssh::Add { id, time }) => {
                let constraints =
                    time.map_or(vec![], |seconds| vec![Constraint::KeyLifetime { seconds }]);
                let id = ssh_add(None, id, keys::prompt::new(), &constraints)?;
                println!("added key for profile id `{}`", id);
            },
            ssh::Options::Rm(ssh::Rm { id }) => {
                let id = ssh_remove(None, id, keys::prompt::new())?;
                println!("removed key for profile id `{}`", id);
            },
            ssh::Options::Test(ssh::Test { id }) => {
                let (id, present) = ssh_test(None, id)?;
                if present {
                    println!("key is on ssh-agent for profile id `{}`", id);
                } else {
                    println!("key is *not* on ssh-agent for profile id `{}`", id);
                }
            },
        },
    }

    Ok(())
}
