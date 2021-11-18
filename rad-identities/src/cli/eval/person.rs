// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::BTreeSet, convert::TryFrom as _, io, path::PathBuf};

use anyhow::anyhow;

use librad::{
    crypto::PublicKey,
    git::{
        identities,
        storage::ReadOnly,
        types::{Namespace, Reference},
        Urn,
    },
    identities::payload,
    profile::Profile,
    PeerId,
};
use rad_clib::{
    keys::ssh::SshAuthSock,
    storage::{self, ssh},
};

use crate::{cli::args::person::*, display, person};

pub fn eval(profile: &Profile, sock: SshAuthSock, opts: Options) -> anyhow::Result<()> {
    match opts {
        Options::Create(CreateOptions { create }) => eval_create(profile, sock, create)?,
        Options::Get(Get { urn, peer }) => eval_get(profile, urn, peer)?,
        Options::List(List {}) => eval_list(profile)?,
        Options::Update(Update {
            urn,
            whoami,
            payload,
            ext,
            delegations,
        }) => eval_update(profile, sock, urn, whoami, payload, ext, delegations)?,
        Options::Checkout(Checkout { urn, path, peer }) => {
            eval_checkout(profile, sock, urn, path, peer)?
        },
        Options::Diff(Diff { urn, peer }) => eval_diff(profile, urn, peer)?,
        Options::Accept(Accept { urn, peer, force }) => {
            eval_accept(profile, sock, urn, peer, force)?
        },
        Options::Tracked(Tracked { urn }) => eval_tracked(profile, urn)?,
    }

    Ok(())
}

fn eval_create(profile: &Profile, sock: SshAuthSock, create: Create) -> anyhow::Result<()> {
    let (signer, storage) = ssh::storage(profile, sock)?;
    let paths = profile.paths();
    let person = match create {
        Create::New(New {
            payload,
            ext,
            delegations,
            path,
        }) => person::create(
            &storage,
            paths.clone(),
            signer,
            payload,
            ext,
            delegations,
            person::Creation::New { path },
        )?,
        Create::Existing(Existing {
            payload,
            ext,
            delegations,
            path,
        }) => person::create(
            &storage,
            paths.clone(),
            signer,
            payload,
            ext,
            delegations,
            person::Creation::Existing { path },
        )?,
    };
    println!("{}", serde_json::to_string(&person::Display::from(person))?);
    Ok(())
}

fn eval_get(profile: &Profile, urn: Urn, peer: Option<PeerId>) -> anyhow::Result<()> {
    let storage = storage::read_only(profile)?;
    let rad = Reference::rad_id(Namespace::from(&urn)).with_remote(peer);
    let urn = Urn::try_from(rad).map_err(|err| anyhow!(err))?;
    let person =
        person::get(&storage, &urn)?.ok_or_else(|| identities::Error::NotFound(urn.clone()))?;
    println!("{}", serde_json::to_string(&person::Display::from(person))?);
    Ok(())
}

fn eval_list(profile: &Profile) -> anyhow::Result<()> {
    let storage = storage::read_only(profile)?;
    let persons = person::list(&storage)?;
    let persons = persons
        .map(|p| p.map(person::Display::from))
        .collect::<Result<Vec<_>, _>>()?;
    println!("{}", serde_json::to_string(&persons)?);
    Ok(())
}

fn eval_update(
    profile: &Profile,
    sock: SshAuthSock,
    urn: Urn,
    whoami: Option<Urn>,
    payload: Option<payload::Person>,
    ext: Vec<payload::Ext<serde_json::Value>>,
    delegations: Vec<PublicKey>,
) -> anyhow::Result<()> {
    let (_, storage) = ssh::storage(profile, sock)?;
    let person = person::update(
        &storage,
        &urn,
        whoami,
        payload,
        ext,
        if delegations.is_empty() {
            None
        } else {
            Some(delegations.into_iter())
        },
    )?;
    println!("{}", serde_json::to_string(&person::Display::from(person))?);
    Ok(())
}

fn eval_checkout(
    profile: &Profile,
    sock: SshAuthSock,
    urn: Urn,
    path: PathBuf,
    peer: Option<PeerId>,
) -> anyhow::Result<()> {
    let paths = profile.paths();
    let (signer, storage) = ssh::storage(profile, sock)?;
    let repo = person::checkout(&storage, paths.clone(), signer, &urn, peer, path)?;
    println!("working copy created at `{}`", repo.path().display());
    Ok(())
}

fn eval_tracked(profile: &Profile, urn: Urn) -> anyhow::Result<()> {
    let storage = storage::read_only(profile)?;
    let peers = person::tracked(&storage, &urn)?
        .into_iter()
        .map(|peer| peer.map(|status| status.map(display::Persona::from)))
        .collect::<Vec<_>>();
    println!("{}", serde_json::to_string(&peers)?);
    Ok(())
}

fn eval_diff(profile: &Profile, urn: Urn, peer: PeerId) -> anyhow::Result<()> {
    let storage = storage::read_only(profile)?;
    diff(&storage, urn, peer)?;
    Ok(())
}

fn eval_accept(
    profile: &Profile,
    sock: SshAuthSock,
    urn: Urn,
    peer: PeerId,
    force: bool,
) -> anyhow::Result<()> {
    let (_, storage) = storage::ssh::storage(profile, sock)?;

    diff(&storage, urn.clone(), peer)?;

    let accept = || -> anyhow::Result<()> {
        let person = identities::person::merge(&storage, &urn, peer)?;
        println!("{}", serde_json::to_string(&person::Display::from(person))?);
        Ok(())
    };

    let accept_loop = || -> anyhow::Result<()> {
        use std::io::Write as _;

        let yes: BTreeSet<String> = vec!["y", "yes"]
            .into_iter()
            .map(|y| y.to_string())
            .collect();
        let no: BTreeSet<String> = vec!["n", "no"].into_iter().map(|n| n.to_string()).collect();

        print!("Would like to accept these changes [yes/no]?: ");
        io::stdout().flush()?;
        let answer = {
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;

            input.trim().to_ascii_lowercase()
        };

        loop {
            if yes.contains(&answer) {
                accept()?;
                break;
            } else if no.contains(&answer) {
                println!("not accepting changes");
                break;
            } else {
                println!("invalid choice");
            }
        }

        Ok(())
    };

    if force {
        return accept();
    } else {
        accept_loop()?;
    }

    Ok(())
}

fn diff<S>(storage: &S, urn: Urn, peer: PeerId) -> anyhow::Result<()>
where
    S: AsRef<ReadOnly>,
{
    let storage = storage.as_ref();
    let local = storage.peer_id();
    let ours = identities::person::get(&storage, &urn)?
        .ok_or_else(|| person::Error::Identities(identities::Error::NotFound(urn.clone())))?;
    let theirs = {
        let urn = Urn::try_from(Reference::rad_id(Namespace::from(&urn)).with_remote(peer))
            .expect("namespace is set");
        identities::person::get(&storage, &urn)?
            .ok_or(person::Error::Identities(identities::Error::NotFound(urn)))?
    };

    let ours = &serde_json::to_string_pretty(&ours.payload()).unwrap();
    let theirs = &serde_json::to_string_pretty(&theirs.payload()).unwrap();

    println!(
        "{}",
        similar::TextDiff::from_lines(ours, theirs)
            .unified_diff()
            .context_radius(10)
            .header(&format!("ours @ {}", local), &format!("theirs @ {}", peer))
    );
    Ok(())
}
