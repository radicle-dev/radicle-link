// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

#![feature(str_strip)]

extern crate radicle_keystore as keystore;

use std::{
    env,
    fs::{self, Permissions},
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use futures::executor::block_on;
use keystore::{pinentry::SecUtf8, Keystore};
use tempfile::tempdir;

use librad::{
    git::{
        local::{self, url::LocalUrl},
        storage::Storage,
    },
    keys::{PublicKey, SecretKey},
    meta::entity::Signatory,
    paths::Paths,
    uri::RadUrn,
};
use librad_test::{
    logging,
    rad::entity::{Alice, Radicle},
};

const PASSPHRASE: &str = "123";

#[test]
fn smoke() {
    logging::init();

    let rad_dir = tempdir().unwrap();
    let rad_paths = Paths::from_root(rad_dir.path()).unwrap();
    let key = SecretKey::new();

    let urn = block_on(setup_entity(&rad_paths, key.clone())).unwrap();
    setup_keystore(rad_paths.keys_dir(), key).unwrap();
    let path = setup_path().unwrap();

    let credentials_cache_dir = tempdir().unwrap();
    fs::set_permissions(credentials_cache_dir.path(), Permissions::from_mode(0o700)).unwrap();
    let credentials_cache_socket = credentials_cache_dir.path().join("socket");

    // Push something to `urn`
    {
        let repo_dir = tempdir().unwrap();
        setup_repo(repo_dir.path(), &credentials_cache_socket, &urn).unwrap();

        let mut child = Command::new("git")
            .args(&["push", "origin", "master"])
            .current_dir(repo_dir.path())
            .env("PATH", &path)
            .env("RAD_HOME", rad_dir.path())
            .env("GIT_DIR", repo_dir.path().join(".git"))
            .envs(env::vars().filter(|(key, _)| key.starts_with("GIT_TRACE")))
            .spawn()
            .unwrap();

        let status = child.wait().unwrap();
        assert!(status.success())
    }

    // Clone from `urn` into a fresh repo
    {
        let repo_dir = tempdir().unwrap();
        let mut child = Command::new("git")
            .arg("-c")
            .arg(format!(
                "credential.helper=cache --socket {}",
                credentials_cache_socket.to_string_lossy()
            ))
            .arg("clone")
            .arg(LocalUrl::from(urn).to_string())
            .arg(repo_dir.path())
            .env("PATH", &path)
            .env("RAD_HOME", rad_dir.path())
            .envs(env::vars().filter(|(key, _)| key.starts_with("GIT_TRACE")))
            .spawn()
            .unwrap();

        let status = child.wait().unwrap();
        assert!(status.success())
    }
}

async fn setup_entity(paths: &Paths, key: SecretKey) -> anyhow::Result<RadUrn> {
    let mut alice = Alice::new(key.public());
    let mut radicle = Radicle::new(&alice);
    {
        let resolves_to_alice = alice.clone();
        alice
            .sign(&key, &Signatory::OwnedKey, &resolves_to_alice)
            .await?;
        radicle
            .sign(&key, &Signatory::User(alice.urn()), &resolves_to_alice)
            .await?;

        let store = Storage::open_or_init(&paths, key)?;
        store.create_repo(&alice)?;
        store.create_repo(&radicle)?;
    }

    Ok(radicle.urn())
}

fn setup_keystore(dir: &Path, key: SecretKey) -> anyhow::Result<()> {
    let mut keystore = keystore::FileStorage::<_, PublicKey, _, _>::new(
        &dir.join("librad.key"),
        keystore::crypto::Pwhash::new(SecUtf8::from(PASSPHRASE.as_bytes())),
    );
    keystore.put_key(key)?;

    Ok(())
}

fn setup_repo(path: &Path, credentials_cache_socket: &Path, origin: &RadUrn) -> anyhow::Result<()> {
    let repo = git2::Repository::init(path)?;
    let blob = repo.blob(b"do you know who I am?")?;
    let tree = {
        let mut builder = repo.treebuilder(None)?;
        builder.insert("README", blob, 0o100_644)?;
        let oid = builder.write()?;
        repo.find_tree(oid)
    }?;
    let author = git2::Signature::now("Charlie H.", "ch@iohk.io")?;
    repo.commit(
        Some("refs/heads/master"),
        &author,
        &author,
        "Initial commit",
        &tree,
        &[],
    )?;

    repo.set_head("refs/heads/master")?;
    repo.remote("origin", &LocalUrl::from(origin).to_string())?;

    let mut config = repo.config()?;
    setup_credential_cache(repo.path(), credentials_cache_socket, &mut config, origin)
}

fn setup_credential_cache(
    git_dir: &Path,
    credentials_cache_socket: &Path,
    config: &mut git2::Config,
    urn: &RadUrn,
) -> anyhow::Result<()> {
    config.set_str(
        "credential.helper",
        &format!(
            "cache --timeout=10 --socket {}",
            credentials_cache_socket.display()
        ),
    )?;
    let mut child = Command::new("git")
        .env("GIT_DIR", git_dir)
        .envs(::std::env::vars().filter(|(key, _)| key.starts_with("GIT_TRACE")))
        .args(&["credential", "approve"])
        .stdin(Stdio::piped())
        .stderr(Stdio::inherit())
        .stdout(Stdio::inherit())
        .spawn()?;
    {
        let stdin = child.stdin.as_mut().expect("could not obtain stdin");
        stdin.write_all(
            format!(
                "protocol={}\nhost={}\nusername=radicle\npassword={}",
                local::URL_SCHEME,
                urn.id,
                PASSPHRASE
            )
            .as_bytes(),
        )?;
    }
    let status = child.wait()?;
    if !status.success() {
        return Err(anyhow::anyhow!("failed to cache credential"));
    }

    Ok(())
}

fn setup_path() -> anyhow::Result<PathBuf> {
    let helper_path = env!("CARGO_BIN_EXE_git-remote-rad");
    let helper_path = Path::new(helper_path.strip_suffix("git-remote-rad").unwrap());
    let path = match env::var_os("PATH") {
        None => env::join_paths(Some(helper_path)),
        Some(path) => {
            let mut paths = env::split_paths(&path).collect::<Vec<_>>();
            paths.push(helper_path.to_path_buf());
            paths.reverse();
            env::join_paths(paths)
        },
    }?;

    Ok(PathBuf::from(path))
}
