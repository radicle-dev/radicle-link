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

extern crate radicle_keystore as keystore;

use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

use keystore::{crypto, pinentry::SecUtf8, Keystore};
use tempfile::tempdir;

use librad::{
    git::{local::url::LocalUrl, storage::Storage},
    keys::{PublicKey, SecretKey},
    meta::entity::Signatory,
    paths::Paths,
    peer::PeerId,
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
    let peer_id = PeerId::from(key);

    let urn = setup_entity(&rad_paths, key).unwrap();
    setup_keystore(rad_paths.keys_dir(), key).unwrap();
    let path = setup_path().unwrap();

    // Push something to `urn`
    {
        let repo_dir = tempdir().unwrap();
        setup_repo(repo_dir.path(), &urn, peer_id).unwrap();

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
            .arg(format!("credential.helper={}", credential_helper()))
            .arg("clone")
            .arg(LocalUrl::from_urn(urn, peer_id).to_string())
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

fn setup_entity(paths: &Paths, key: SecretKey) -> anyhow::Result<RadUrn> {
    let mut alice = Alice::new(key.public());
    let mut radicle = Radicle::new(&alice);
    {
        let resolves_to_alice = alice.clone();
        alice.sign(&key, &Signatory::OwnedKey, &resolves_to_alice)?;
        radicle.sign(&key, &Signatory::User(alice.urn()), &resolves_to_alice)?;

        let store = Storage::open_or_init(&paths, key)?;
        store.create_repo(&alice)?;
        store.create_repo(&radicle)?;
        store.set_default_rad_self(
            (*alice)
                .clone()
                .check_history_status(&resolves_to_alice, &resolves_to_alice)?,
        )?;
    }

    Ok(radicle.urn())
}

fn setup_keystore(dir: &Path, key: SecretKey) -> anyhow::Result<()> {
    // Nb. We need to use the prod KDF params here, because the only way to test
    // the remote helper executable is via an integration test, and we don't
    // have any way to cfg(test) the library under test.
    let crypto = crypto::Pwhash::new(SecUtf8::from(PASSPHRASE), *crypto::KDF_PARAMS_PROD);
    let mut keystore =
        keystore::FileStorage::<_, PublicKey, _, _>::new(&dir.join("librad.key"), crypto);
    keystore.put_key(key)?;

    Ok(())
}

fn setup_repo(path: &Path, origin: &RadUrn, peer_id: PeerId) -> anyhow::Result<()> {
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
    repo.remote(
        "origin",
        &LocalUrl::from_urn(origin.clone(), peer_id).to_string(),
    )?;

    let mut config = repo.config()?;
    config
        .set_str("credential.helper", &credential_helper())
        .map_err(|e| e.into())
}

fn credential_helper() -> String {
    format!(
        "!f() {{ test \"$1\" = get && echo \"password={}\"; }}; f",
        PASSPHRASE
    )
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
