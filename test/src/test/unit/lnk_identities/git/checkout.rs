// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::ops::Index as _;

use either::Either;
use tempfile::tempdir;

use librad::{
    canonical::Cstring,
    crypto::SecretKey,
    git::{
        identities::local,
        local::{transport, url::LocalUrl},
        util,
        Storage,
    },
    git_ext::tree,
    reflike,
    PeerId,
};
use lnk_identities::git::checkout::*;

use crate::{
    librad::paths::paths,
    rad::{
        identities::{TestPerson, TestProject},
        testnet,
    },
};

#[test]
fn local_checkout() -> anyhow::Result<()> {
    let temp = tempdir()?;
    let paths = paths();
    let signer = SecretKey::new();
    let storage = Storage::open(&*paths, signer.clone())?;
    let proj = TestProject::create(&storage)?;
    let urn = proj.project.urn().with_path(reflike!("refs/heads/next"));
    util::quick_commit(
        &storage,
        &urn,
        vec![("HI", tree::blob(b"Hi Bob"))].into_iter().collect(),
        "say hi to bob",
    )?;
    let settings = transport::Settings {
        paths: paths.clone(),
        signer: signer.into(),
    };

    let local = Local::new(&proj.project, temp.path().to_path_buf());
    let repo = checkout(settings, &proj.project, Either::Left(local))?;
    let branch = proj.project.subject().default_branch.as_ref().unwrap();
    assert_head(&repo, branch)?;
    assert_remote(&repo, branch, &LocalUrl::from(proj.project.urn()))?;
    Ok(())
}

#[test]
fn remote_checkout() {
    let net = testnet::run(config()).unwrap();
    net.enter(async {
        let peer1 = net.peers().index(0);
        let peer2 = net.peers().index(1);
        let temp = tempdir().unwrap();

        let proj = peer1
            .using_storage(TestProject::create)
            .await
            .unwrap()
            .unwrap();
        peer1
            .using_storage({
                let urn = proj.project.urn().with_path(reflike!("refs/heads/next"));
                move |storage| {
                    util::quick_commit(
                        storage,
                        &urn,
                        vec![("HI", tree::blob(b"Hi Bob"))].into_iter().collect(),
                        "say hi to bob",
                    )
                }
            })
            .await
            .unwrap()
            .unwrap();
        proj.pull(peer1, peer2).await.unwrap();
        peer2
            .using_storage({
                move |storage| -> anyhow::Result<()> {
                    let mut config = storage.config()?;
                    let person = TestPerson::create(storage)?;
                    let local = local::load(storage, person.owner.urn())?.unwrap();
                    Ok(config.set_user(local)?)
                }
            })
            .await
            .unwrap()
            .unwrap();

        let settings = transport::Settings {
            paths: peer2.protocol_config().paths.clone(),
            signer: peer2.signer().clone().into(),
        };

        let remote = (proj.owner.clone(), peer1.peer_id());
        let peer = Peer::new(&proj.project, remote, temp.path().to_path_buf()).unwrap();
        let repo = checkout(settings, &proj.project, Either::Right(peer)).unwrap();
        let branch = proj.project.subject().default_branch.as_ref().unwrap();
        assert_head(&repo, branch).unwrap();
        assert_remote(&repo, branch, &LocalUrl::from(proj.project.urn())).unwrap();
        assert_peer_remote(
            &repo,
            branch,
            &proj.owner.subject().name,
            &peer1.peer_id(),
            &LocalUrl::from(proj.project.urn()),
        )
        .unwrap();
    })
}

fn config() -> testnet::Config {
    testnet::Config {
        num_peers: nonzero!(2usize),
        min_connected: 2,
        bootstrap: testnet::Bootstrap::from_env(),
    }
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

/// Assert that:
///   * the peer remote exists
///   * its URL matches the `LocalUrl`
///   * the refs/remotes/<handle>@<peer>/<branch> exists
fn assert_peer_remote(
    repo: &git2::Repository,
    branch: &Cstring,
    handle: &Cstring,
    peer: &PeerId,
    url: &LocalUrl,
) -> anyhow::Result<()> {
    let remote_name = format!("{}@{}", handle, peer);
    let remote = repo.find_remote(&remote_name)?;
    assert_eq!(remote.url().unwrap(), &url.to_string());

    let branch = repo.find_branch(
        &format!("{}/{}", remote_name, branch),
        git2::BranchType::Remote,
    );
    assert!(branch.is_ok());

    Ok(())
}
