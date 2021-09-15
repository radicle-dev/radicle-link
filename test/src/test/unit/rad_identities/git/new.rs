// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::fs::{self, File};

use tempfile::tempdir;

use librad::{
    canonical::Cstring,
    crypto::SecretKey,
    git::{
        identities::project::ProjectPayload,
        local::{transport, url::LocalUrl},
        storage::Storage,
    },
};
use rad_identities::git::new::*;

use crate::{
    librad::paths::paths,
    rad::identities::{radicle_link, TestProject},
};

#[test]
fn validation_path_is_file() -> anyhow::Result<()> {
    let payload = radicle_link();
    let temp = tempdir()?;
    let file = temp.path().join(payload.name.as_str());
    let _ = File::create(file)?;
    let new = New::new(ProjectPayload::new(payload), temp.path().to_path_buf());
    let result = new.validate();
    assert_matches!(result, Err(Error::AlreadyExists(_)));
    Ok(())
}

#[test]
fn validation_path_is_directory() -> anyhow::Result<()> {
    let payload = radicle_link();
    let temp = tempdir()?;
    let dir = temp.path().join(payload.name.as_str());
    fs::create_dir(dir.clone())?;
    let _ = File::create(dir.join("existing_file"));
    let new = New::new(ProjectPayload::new(payload), temp.path().to_path_buf());
    let result = new.validate();
    assert_matches!(result, Err(Error::AlreadyExists(_)));
    Ok(())
}

#[test]
fn creation() -> anyhow::Result<()> {
    let payload = radicle_link();
    let temp = tempdir()?;
    let new = New::new(
        ProjectPayload::new(payload.clone()),
        temp.path().to_path_buf(),
    );
    let validated = new.validate()?;

    let (url, repo) = {
        let paths = paths();
        let signer = SecretKey::new();
        let storage = Storage::open(&*paths, signer.clone())?;
        let proj = TestProject::create(&storage)?;
        let urn = proj.project.urn();
        let url = LocalUrl::from(urn);
        let settings = transport::Settings {
            paths: paths.clone(),
            signer: signer.into(),
        };

        (url.clone(), validated.init(url, settings)?)
    };

    let branch = payload.default_branch.unwrap();
    assert_eq!(
        repo.path().canonicalize()?,
        temp.path()
            .join(payload.name.as_str())
            .join(".git")
            .canonicalize()?
    );
    assert_head(&repo, &branch)?;
    assert_remote(&repo, &branch, &url)?;

    Ok(())
}

/// Assert that:
///  * HEAD exists
///  * the name of HEAD is the default branch
///  * HEAD peels to a commit
fn assert_head(repo: &git2::Repository, branch: &Cstring) -> anyhow::Result<()> {
    let head = repo.head()?;
    let name = head.name().unwrap();
    let expected = format!("refs/heads/{}", branch);
    assert_eq!(name, expected);

    let commit = head.peel_to_commit();
    assert!(commit.is_ok());

    Ok(())
}

/// Assert that:
///   * the `rad` remote exists
///   * its URL matches the `LocalUrl`
///   * its upstream branch is the default branch
fn assert_remote(repo: &git2::Repository, branch: &Cstring, url: &LocalUrl) -> anyhow::Result<()> {
    let rad = repo.find_remote("rad")?;
    assert_eq!(rad.url().unwrap(), &url.to_string());

    let local = repo.find_branch(branch.as_str(), git2::BranchType::Local)?;
    let upstream = local.upstream()?;
    let name = upstream.name()?.unwrap();
    let expected = format!("rad/{}", branch);
    assert_eq!(name, expected);

    Ok(())
}
