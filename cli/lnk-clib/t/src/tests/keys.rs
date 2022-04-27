// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use tempfile::tempdir;

use it_helpers::ssh::with_ssh_agent;
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
use test_helpers::logging;

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
        ssh::add_signer(&profile, sock.clone(), pass, Vec::new())?;
        let signer = ssh::signer(&profile, sock)?;
        let sig = signer.sign_blocking(b"secret message")?;
        Ok((sig, signer.peer_id()))
    })?;

    let pk = peer_id.as_public_key();
    assert!(pk.verify(&sig.into(), b"secret message"));

    Ok(())
}

#[test]
fn async_agent_signature() -> anyhow::Result<()> {
    // This test reproduces an issue which caused the SshSigner to block the tokio
    // runtime. The scenario where this happens is when the `block_on` in the
    // `Signer::sign_blocking` occurs on the tokio worker thread which is
    // driving IO. In this case the IO driver freezes so the tokio
    // runtime cannot make progress.
    //
    // To cause this to happen we need to spawn a task which is waiting on some IO
    // and which will call `sign_blocking` once the IO it is waiting for is
    // ready. To achieve this first we create a socket and then spawn a thread
    // which will write to the socket after sleeping, then we spawn a tokio task
    // which waits for input from the thread and then calls
    // `Signer::sign_blocking`
    logging::init();

    let temp = tempdir()?;
    let pass = Pwhash::new(SecUtf8::from(b"42".to_vec()), *KDF_PARAMS_TEST);
    let home = LnkHome::Root(temp.path().to_path_buf());
    let id = ProfileId::new();
    let profile = Profile::from_home(&home, Some(id))?;
    let key = SecretKey::new();
    let peer_id = librad::PeerId::from(key.clone());
    let mut key_store = file_storage(&profile, pass.clone());
    key_store.put_key(key.clone())?;
    let _ = Storage::open(profile.paths(), key)?;

    // Create the socket
    let testsock_dir = tempfile::tempdir()?;
    let sock_path = testsock_dir.path().join("sox");
    let sock2 = sock_path.clone();

    // Create the thread which writes to the socket after one second
    let send_thread = std::thread::spawn(move || {
        use std::io::Write;
        std::thread::sleep(std::time::Duration::from_secs(1));
        let mut stream = std::os::unix::net::UnixStream::connect(sock2).unwrap();
        stream.write_all(b"hello\n").unwrap();
    });

    let sig = with_ssh_agent(|sock| {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let _guard = rt.enter();

        ssh::add_signer(&profile, sock.clone(), pass, Vec::new())?;
        let signer = ssh::signer(&profile, sock)?;

        // Spawn the task which waits on IO, and then calls `sign_blocking`
        let sig_task = rt.spawn(async move {
            let listener = tokio::net::UnixListener::bind(sock_path)?;
            let (_stream, _) = listener.accept().await?;
            signer
                .sign_blocking(b"secret message")
                .map_err(anyhow::Error::from)
        });
        rt.block_on(async { sig_task.await }).unwrap()
    })?;

    send_thread.join().unwrap();

    let pk = peer_id.as_public_key();
    assert!(pk.verify(&sig.into(), b"secret message"));

    Ok(())
}
