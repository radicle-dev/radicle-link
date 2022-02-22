// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use tempfile::tempdir;

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
    profile::{LnkHome, Profile, ProfileId},
    Signer as _,
};
use lnk_clib::keys::{file_storage, ssh};

use crate::{logging, ssh::with_ssh_agent};

#[test]
fn agent_signature() -> anyhow::Result<()> {
    logging::init();

    let temp = tempdir()?;
    let pass = Pwhash::new(SecUtf8::from(b"42".to_vec()), *KDF_PARAMS_TEST);
    let home = LnkHome::Root(temp.path().to_path_buf());
    let id = ProfileId::new();
    let profile = Profile::from_home(&home, Some(id))?;
    let key = SecretKey::new();
    let mut key_store = file_storage(&profile, pass.clone());
    key_store.put_key(key.clone())?;
    let _ = Storage::open(profile.paths(), key)?;

    let (sig, peer_id) = with_ssh_agent(|sock| {
        ssh::add_signer(&profile, sock.clone(), pass, &[])?;
        let signer = ssh::signer(&profile, sock)?;
        let sig = signer.sign_blocking(b"secret message")?;
        Ok((sig, signer.peer_id()))
    })?;

    let pk = peer_id.as_public_key();
    assert!(pk.verify(&sig.into(), b"secret message"));

    Ok(())
}
