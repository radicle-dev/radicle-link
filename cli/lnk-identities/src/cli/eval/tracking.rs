// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::{
    git::tracking::{git::refdb::PrunedRef, policy, UntrackArgs, Untracked},
    profile::Profile,
};
use lnk_clib::{keys::ssh::SshAuthSock, storage::ssh};

use crate::{cli::args::tracking::*, tracking};

pub fn eval_track(
    profile: &Profile,
    sock: SshAuthSock,
    Track {
        urn,
        peer,
        config,
        force,
    }: Track,
) -> anyhow::Result<()> {
    let (_, storage) = ssh::storage(profile, sock)?;
    let paths = profile.paths();
    let policy = if force {
        policy::Track::Any
    } else {
        policy::Track::MustNotExist
    };
    match tracking::track(&storage, paths, &urn, peer, config, policy)? {
        Ok(r) => println!("created tracking relationship `{}`", r.name),
        Err(err) => eprintln!("could not create tracking relationship: {}", err),
    }
    Ok(())
}

pub fn eval_untrack(
    profile: &Profile,
    sock: SshAuthSock,
    Untrack { urn, peer, prune }: Untrack,
) -> anyhow::Result<()> {
    let (_, storage) = ssh::storage(profile, sock)?;
    let paths = profile.paths();
    let args = UntrackArgs {
        policy: policy::Untrack::Any,
        prune,
    };
    match tracking::untrack(&storage, paths, &urn, peer, args)? {
        Ok(Untracked { pruned, .. }) => {
            println!("untracked `{}` for `{}`", urn, peer);
            if let Some(pruned) = pruned {
                for prune in pruned.0 {
                    match prune {
                        PrunedRef::Direct { name, .. } => println!("pruned reference `{}`", name),
                        PrunedRef::Symbolic { name } => {
                            println!("pruned symbolic reference `{}`", name)
                        },
                    }
                }
            }
        },
        // NOTE: this shouldn't be possible because of the use of `Untrack::Any` above, but keeping
        // it here for posterity.
        Err(err) => eprintln!("could not remove tracking relationship: {}", err),
    }
    Ok(())
}
