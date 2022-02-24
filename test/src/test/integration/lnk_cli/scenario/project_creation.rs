// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use tempfile::tempdir;
use thrussh_agent::Constraint;

use librad::{
    crypto::keystore::{
        crypto::{Pwhash, KDF_PARAMS_TEST},
        pinentry::SecUtf8,
    },
    identities::payload,
    profile::{LnkHome, Profile},
};
use lnk_clib::{keys::ssh::SshAuthSock, storage};
use lnk_identities as identities;
use lnk_profile as profile;
use test_helpers::logging;

use crate::ssh::with_ssh_agent;

fn set_local_identity(profile: &Profile, sock: SshAuthSock) -> anyhow::Result<()> {
    use lnk_identities::cli::args::{local, person};
    identities::cli::eval::person::eval(
        profile,
        sock.clone(),
        person::Options::Create(person::CreateOptions {
            create: person::Create::New(person::New {
                payload: payload::Person {
                    name: "Ralph Wiggums".into(),
                },
                ext: vec![],
                delegations: vec![],
                path: None,
            }),
        }),
    )?;

    // We expect only one person to be in the storage
    let person = {
        let storage = storage::read_only(profile)?;
        let mut persons = identities::person::list(&storage)?;
        let person = persons.next().unwrap();
        person?
    };

    identities::cli::eval::local::eval(
        profile,
        sock,
        local::Options::Set(local::Set { urn: person.urn() }),
    )?;

    Ok(())
}

#[test]
fn create() -> anyhow::Result<()> {
    use lnk_identities::cli::args::project::*;

    logging::init();

    let temp = tempdir()?;
    let pass = Pwhash::new(SecUtf8::from(b"42".to_vec()), *KDF_PARAMS_TEST);
    let home = LnkHome::Root(temp.path().to_path_buf());
    let (profile, _) = profile::create(home.clone(), pass.clone())?;

    with_ssh_agent(|sock| {
        profile::ssh_add(
            home,
            profile.id().clone(),
            sock.clone(),
            pass,
            &[Constraint::KeyLifetime { seconds: 10 }],
        )?;

        // create local identity
        set_local_identity(&profile, sock.clone())?;

        // create project
        identities::cli::eval::project::eval(
            &profile,
            sock,
            Options::Create(CreateOptions {
                create: Create::New(New {
                    payload: payload::Project {
                        name: "Simpson's Road Rage".into(),
                        default_branch: Some("main".into()),
                        description: None,
                    },
                    whoami: None,
                    ext: vec![],
                    delegations: vec![],
                    path: None,
                }),
            }),
        )
    })?;
    Ok(())
}
