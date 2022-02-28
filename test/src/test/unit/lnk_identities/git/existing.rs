// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom as _, fs, path::Path};

use tempfile::tempdir;

use librad::{
    canonical::Cstring,
    crypto::SecretKey,
    git::{
        identities::project::ProjectPayload,
        local::{transport, url::LocalUrl},
        storage::Storage,
        types::remote::Remote,
        Urn,
    },
    git_ext::RefLike,
    reflike,
};
use lnk_identities::git::{self, existing::*, new::New};

use crate::{
    git::create_commit,
    librad::paths::paths,
    rad::identities::{radicle_link, TestProject},
};

#[test]
fn validation_missing_path() -> anyhow::Result<()> {
    let payload = radicle_link();
    let existing = Existing::new(
        ProjectPayload::new(payload),
        Path::new("missing").to_path_buf(),
    );
    let result = existing.validate();
    assert_matches!(result, Err(Error::PathDoesNotExist(_)));
    Ok(())
}

#[test]
fn validation_path_is_not_a_repo() -> anyhow::Result<()> {
    let payload = radicle_link();
    let temp = tempdir()?;
    let dir = temp.path().join(payload.name.as_str());
    fs::create_dir(dir)?;
    let existing = Existing::new(ProjectPayload::new(payload), temp.path().to_path_buf());
    let result = existing.validate();
    assert_matches!(result, Err(Error::NotARepo(_)));
    Ok(())
}

#[test]
fn validation_default_branch_is_missing() -> anyhow::Result<()> {
    let payload = radicle_link();
    let temp = tempdir()?;
    let dir = temp.path().join(payload.name.as_str());
    let _repo = git2::Repository::init(dir)?;
    let existing = Existing::new(ProjectPayload::new(payload), temp.path().to_path_buf());
    let result = existing.validate();
    assert_matches!(
        result,
        Err(Error::Validation(
            git::validation::Error::MissingDefaultBranch { .. }
        ))
    );
    Ok(())
}

#[test]
fn validation_different_remote_exists() -> anyhow::Result<()> {
    let payload = radicle_link();
    let temp = tempdir()?;
    let dir = temp.path().join(payload.name.as_str());
    let _repo = {
        let branch = payload.default_branch.as_ref().unwrap();
        let mut opts = git2::RepositoryInitOptions::new();
        opts.initial_head(branch.as_str());
        let repo = git2::Repository::init_opts(dir, &opts)?;

        create_commit(
            &repo,
            reflike!("refs/heads").join(RefLike::try_from(branch.as_str())?),
        )?;

        let urn = Urn::new(git2::Oid::zero().into());
        let url = LocalUrl::from(urn);
        let mut remote = Remote::new(url, reflike!("rad"));
        remote.save(&repo)?;

        repo
    };
    let existing = Existing::new(ProjectPayload::new(payload), temp.path().to_path_buf());
    let result = {
        let valid = existing.validate()?;
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

        valid.init(url, settings)
    };
    assert_matches!(
        result.err(),
        Some(Error::Validation(
            git::validation::Error::UrlMismatch { .. }
        ))
    );
    Ok(())
}

#[test]
fn validation_remote_exists() -> anyhow::Result<()> {
    let payload = radicle_link();
    let temp = tempdir()?;
    let paths = paths();
    let signer = SecretKey::new();
    let storage = Storage::open(&*paths, signer.clone())?;

    let (url, settings, _repo) = {
        let new = New::new(
            ProjectPayload::new(payload.clone()),
            temp.path().to_path_buf(),
        );
        let validated = new.validate()?;
        let proj = TestProject::create(&storage)?;
        let urn = proj.project.urn();
        let url = LocalUrl::from(urn);
        let settings = transport::Settings {
            paths: paths.clone(),
            signer: signer.into(),
        };

        (
            url.clone(),
            settings.clone(),
            validated.init(url, settings)?,
        )
    };
    let existing = Existing::new(payload, temp.path().to_path_buf());
    let result = {
        let valid = existing.validate()?;
        valid.init(url, settings)
    };
    assert!(result.is_ok(), "{:?}", result.err());
    Ok(())
}

#[test]
fn creation() -> anyhow::Result<()> {
    let payload = radicle_link();
    let temp = tempdir()?;
    let dir = temp.path().join(payload.name.as_str());
    let _repo = {
        let branch = payload.default_branch.as_ref().unwrap();
        let mut opts = git2::RepositoryInitOptions::new();
        opts.initial_head(branch.as_str());
        let repo = git2::Repository::init_opts(dir, &opts)?;
        create_commit(
            &repo,
            reflike!("refs/heads").join(RefLike::try_from(branch.as_str())?),
        )?;
        repo
    };
    let existing = Existing::new(payload.clone(), temp.path().to_path_buf());
    let (url, repo) = {
        let valid = existing.validate()?;
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

        (url.clone(), valid.init(url, settings)?)
    };
    let branch = payload.default_branch.unwrap();
    assert_remote(&repo, &branch, &url)?;
    Ok(())
}

/// Assert that:
///   * the `rad` remote exists
///   * its URL matches the `LocalUrl`
///   * the default branch exists under the remote
fn assert_remote(repo: &git2::Repository, branch: &Cstring, url: &LocalUrl) -> anyhow::Result<()> {
    let rad = repo.find_remote("rad")?;
    assert_eq!(rad.url().unwrap(), &url.to_string());

    let rad_branch = format!("rad/{}", branch);
    let remote_branch = repo.find_branch(&rad_branch, git2::BranchType::Remote);
    assert!(remote_branch.is_ok(), "{:?}", remote_branch.err());
    Ok(())
}
