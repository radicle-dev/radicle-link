// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryInto as _, process::exit};

use agent::Constraint;

use librad::crypto::keystore::sign;
use lnk_clib::keys::{self, ssh::SshAuthSock};

use crate::{
    create, get, list, paths, peer_id, set, ssh_add, ssh_ready, ssh_remove, ssh_sign, ssh_verify,
};

use super::args::*;

pub fn main(Args { command }: Args, sock: SshAuthSock) -> anyhow::Result<()> {
    eval(sock, command)
}

fn eval(sock: SshAuthSock, command: Command) -> anyhow::Result<()> {
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
                    "no active profile found, perhaps you want to run `lnk profile create`?"
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
                let id = ssh_add(None, id, sock, keys::prompt::new(), constraints)?;
                println!("added key for profile id `{}`", id);
            },
            ssh::Options::Rm(ssh::Rm { id }) => {
                let id = ssh_remove(None, id, sock, keys::prompt::new())?;
                println!("removed key for profile id `{}`", id);
            },
            ssh::Options::Sign(ssh::Sign { id, payload }) => {
                let (id, sig) = ssh_sign(None, id, sock, payload)?;
                println!("`{}` signature for profile id `{}`", sig, id);
            },
            ssh::Options::Ready(ssh::Ready { id }) => {
                let (id, present) = ssh_ready(None, id, sock)?;
                if present {
                    println!("key is on ssh-agent for profile id `{}`", id);
                } else {
                    println!("key is *not* on ssh-agent for profile id `{}`", id);
                    exit(1);
                }
            },
            ssh::Options::Verify(ssh::Verify {
                id,
                payload,
                signature,
            }) => {
                let signature: [u8; 64] = signature.as_bytes().try_into()?;
                let signature = sign::Signature(signature);
                let (id, verified) = ssh_verify(None, id, payload, signature.into())?;

                if verified {
                    println!("payload verified for profile id `{}`", id);
                } else {
                    println!("payload *not* verified for profile id `{}`", id);
                }
            },
        },
    }

    Ok(())
}
