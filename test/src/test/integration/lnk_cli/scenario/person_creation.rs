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
    profile::LnkHome,
};
use lnk_identities as identities;
use lnk_profile as profile;

use crate::{logging, ssh::with_ssh_agent};

#[test]
fn create() -> anyhow::Result<()> {
    use lnk_identities::cli::args::person::*;

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
        identities::cli::eval::person::eval(
            &profile,
            sock,
            Options::Create(CreateOptions {
                create: Create::New(New {
                    payload: payload::Person {
                        name: "Ralph Wiggums".into(),
                    },
                    ext: vec![],
                    delegations: vec![],
                    path: None,
                }),
            }),
        )
    })?;
    Ok(())
}
