// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use tempfile::tempdir;

use librad::{
    crypto::keystore::{
        crypto::{Pwhash, KDF_PARAMS_TEST},
        pinentry::SecUtf8,
    },
    identities::payload,
    profile::RadHome,
};
use rad_identities as identities;
use rad_profile as profile;

use crate::logging;

#[test]
fn create() -> anyhow::Result<()> {
    use rad_identities::cli::args::person::*;

    logging::init();

    let temp = tempdir()?;
    let pass = Pwhash::new(SecUtf8::from(b"42".to_vec()), *KDF_PARAMS_TEST);
    let home = RadHome::Root(temp.path().to_path_buf());
    let (profile, _) = profile::create(home.clone(), pass.clone())?;
    profile::ssh_add(home, profile.id().clone(), pass, &[])?;
    identities::cli::eval::person::eval(
        &profile,
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
    )?;
    Ok(())
}
