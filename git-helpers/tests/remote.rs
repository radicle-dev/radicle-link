// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

extern crate radicle_keystore as keystore;

use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

use keystore::{crypto, pinentry::SecUtf8, Keystore};
use tempfile::tempdir;

use librad::{
    git::{local::url::LocalUrl, storage::Storage, Urn},
    keys::{PublicKey, SecretKey},
    paths::Paths,
};
use librad_test::{logging, rad::identities::create_test_project};

const PASSPHRASE: &str = "123";

#[test]
fn smoke() {
    logging::init();

    let rad_dir = tempdir().unwrap();
    let rad_paths = Paths::from_root(rad_dir.path()).unwrap();
    let key = SecretKey::new();

    let urn = setup_project(&rad_paths, key.clone()).unwrap();
    setup_keystore(rad_paths.keys_dir(), key).unwrap();
    let path = setup_path().unwrap();

    // Push something to `urn`
    {
        let repo_dir = tempdir().unwrap();
        setup_repo(repo_dir.path(), &urn).unwrap();

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

fn setup_project(paths: &Paths, key: SecretKey) -> anyhow::Result<Urn> {
    let store = Storage::open_or_init(paths, key)?;
    let proj = create_test_project(&store)?;
    Ok(proj.project.urn())
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

fn setup_repo(path: &Path, origin: &Urn) -> anyhow::Result<()> {
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
    repo.remote("origin", &LocalUrl::from(origin.clone()).to_string())?;

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
