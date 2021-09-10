// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use tempfile::tempdir;
use tokio::net::UnixStream;

use librad::{
    crypto::{
        keystore::{
            crypto::{Pwhash, KDF_PARAMS_TEST},
            pinentry::SecUtf8,
            Keystore as _,
        },
        SecretKey,
    },
    git::storage::Storage,
    profile::{Profile, ProfileId, RadHome},
    Signer as _,
};
use rad_clib::keys::{file_storage, ssh};

use crate::logging;

#[test]
fn agent_signature() -> anyhow::Result<()> {
    logging::init();

    let temp = tempdir()?;
    let pass = Pwhash::new(SecUtf8::from(b"42".to_vec()), *KDF_PARAMS_TEST);
    let home = RadHome::Root(temp.path().to_path_buf());
    let id = ProfileId::new();
    let profile = Profile::from_home(&home, Some(id))?;
    let key = SecretKey::new();
    let mut key_store = file_storage(&profile, pass.clone());
    key_store.put_key(key.clone())?;
    let _ = Storage::open(profile.paths(), key)?;
    ssh::add_signer::<UnixStream, _>(&profile, pass, &[]).unwrap();
    let signer = ssh::signer::<UnixStream>(&profile).unwrap();
    let sig = signer.sign_blocking(b"secret message").unwrap();
    let peer_id = signer.peer_id();
    let pk = peer_id.as_public_key();
    assert!(pk.verify(&sig.into(), b"secret message"));

    Ok(())
}
