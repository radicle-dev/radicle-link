// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryFrom;

use librad::{
    git::{
        local::url::LocalUrl,
        types::{remote::LocalPushspec, Fetchspec, Force, Remote},
    },
    reflike,
};
use pretty_assertions::assert_eq;
use radicle_daemon::{
    project::{peer, Peer},
    seed::Seed,
    state,
    RunConfig,
};
use radicle_git_ext::RefLike;
use test_helpers::logging;

use crate::daemon::common::{assert_fetched, blocking, shia_le_pathbuf, Harness};

#[test]
fn can_clone_project() -> Result<(), anyhow::Error> {
    logging::init();

    let mut harness = Harness::new();
    let alice = harness.add_peer("alice", RunConfig::default(), &[])?;
    let bob = harness.add_peer("bob", RunConfig::default(), &[])?;
    harness.enter(async move {
        let project = state::init_project(
            &alice.peer,
            &alice.owner,
            shia_le_pathbuf(alice.path.join("radicle")),
        )
        .await?;

        state::clone_project(&bob.peer, project.urn(), alice.peer_id, alice.listen_addrs).await?;

        let have = state::list_projects(&bob.peer)
            .await?
            .into_iter()
            .map(|project| project.urn())
            .collect::<Vec<_>>();
        let want = vec![project.urn()];
        assert_eq!(have, want, "bob");

        let another_peer = librad::PeerId::from(librad::crypto::SecretKey::new());
        state::track(&bob.peer, project.urn(), another_peer).await?;
        let mut have = state::tracked(&bob.peer, project.urn())
            .await?
            .into_iter()
            .map(|peer| peer.map(|status| status.map(|user| user.subject().name.to_string())))
            .collect::<Vec<_>>();
        have.sort_by(|p1, p2| p1.status().cmp(p2.status()));
        let want: Vec<_> = vec![
            radicle_daemon::project::Peer::Remote {
                peer_id: another_peer,
                status: peer::Status::NotReplicated,
            },
            radicle_daemon::project::Peer::Remote {
                peer_id: alice.peer_id,
                status: peer::Status::replicated(
                    peer::Role::Maintainer,
                    alice.owner.subject().name.to_string(),
                ),
            },
        ];
        assert_eq!(have, want, "another_peer");

        Ok(())
    })
}

#[test]
fn can_clone_user() -> Result<(), anyhow::Error> {
    logging::init();

    let mut harness = Harness::new();
    let alice = harness.add_peer("alice", RunConfig::default(), &[])?;
    let bob = harness.add_peer("bob", RunConfig::default(), &[])?;
    harness.enter(async move {
        state::clone_user(
            &bob.peer,
            alice.owner.urn(),
            alice.peer_id,
            alice.listen_addrs,
        )
        .await?;

        let alice_urn = alice.owner.urn();
        let has_alice = state::list_users(&bob.peer)
            .await?
            .into_iter()
            .find(|user| user.urn() == alice_urn);
        assert!(has_alice.is_some(), "bob doesn't have alice's default id");

        Ok(())
    })
}

#[test]
fn can_fetch_project_changes() -> Result<(), anyhow::Error> {
    logging::init();

    let mut harness = Harness::new();
    let alice = harness.add_peer("alice", RunConfig::default(), &[])?;
    let bob = harness.add_peer("bob", RunConfig::default(), &[])?;
    harness.enter(async move {
        let alice_repo_path = alice.path.join("radicle");

        let project = state::init_project(
            &alice.peer,
            &alice.owner,
            shia_le_pathbuf(alice_repo_path.clone()),
        )
        .await?;

        state::clone_project(
            &bob.peer,
            project.urn(),
            alice.peer_id,
            alice.listen_addrs.clone(),
        )
        .await
        .expect("unable to clone project");

        assert_eq!(
            state::list_projects(&bob.peer)
                .await?
                .into_iter()
                .map(|project| project.urn())
                .collect::<Vec<_>>(),
            vec![project.urn()]
        );

        let commit_id = blocking({
            let project = project.clone();
            let alice_peer = alice.peer.clone();
            let alice_owner = alice.owner.clone();

            move || {
                let repo = git2::Repository::open(
                    alice_repo_path.join(project.subject().name.to_string()),
                )?;
                let oid = repo
                    .find_reference(&format!(
                        "refs/heads/{}",
                        project.subject().default_branch.clone().unwrap()
                    ))?
                    .target()
                    .expect("Missing first commit");
                let commit = repo.find_commit(oid)?;
                let commit_id = {
                    let empty_tree = {
                        let mut index = repo.index()?;
                        let oid = index.write_tree()?;
                        repo.find_tree(oid)?
                    };

                    let author = git2::Signature::now(
                        &alice_owner.subject().name.to_string(),
                        "alice@example.com",
                    )?;
                    repo.commit(
                        Some(&format!(
                            "refs/heads/{}",
                            project.subject().default_branch.clone().unwrap()
                        )),
                        &author,
                        &author,
                        "Successor commit",
                        &empty_tree,
                        &[&commit],
                    )?
                };

                {
                    let mut rad = Remote::<LocalUrl>::rad_remote::<_, Fetchspec>(
                        LocalUrl::from(project.urn()),
                        None,
                    );
                    let branch = RefLike::try_from(
                        project.subject().default_branch.as_ref().unwrap().as_str(),
                    )
                    .unwrap();
                    let _ = rad.push(
                        state::settings(&alice_peer),
                        &repo,
                        LocalPushspec::Matching {
                            pattern: reflike!("refs/heads").join(branch).into(),
                            force: Force::False,
                        },
                    )?;
                }

                Ok::<_, anyhow::Error>(commit_id)
            }
        })
        .await?;

        state::fetch(&bob.peer, project.urn(), alice.peer_id, alice.listen_addrs).await?;

        let has_commit = state::has_commit(
            &bob.peer,
            project.urn().with_path(Some(
                RefLike::try_from(format!(
                    "refs/remotes/{}/heads/{}",
                    alice.peer_id,
                    project.subject().default_branch.clone().unwrap(),
                ))
                .unwrap(),
            )),
            radicle_daemon::git_ext::Oid::from(commit_id),
        )
        .await?;
        assert!(has_commit, "bob's missing the commit");

        Ok(())
    })
}

#[test]
fn can_create_working_copy_of_peer() -> Result<(), anyhow::Error> {
    logging::init();

    let mut harness = Harness::new();
    let alice = harness.add_peer("alice", RunConfig::default(), &[])?;
    let bob = harness.add_peer("bob", RunConfig::default(), &[])?;
    let eve = harness.add_peer("eve", RunConfig::default(), &[])?;
    harness.enter(async move {
        let project = {
            let project = state::init_project(
                &alice.peer,
                &alice.owner,
                shia_le_pathbuf(alice.path.join("radicle")),
            )
            .await?;

            state::clone_project(&bob.peer, project.urn(), alice.peer_id, alice.listen_addrs)
                .await
                .expect("unable to clone project");
            state::clone_project(
                &eve.peer,
                project.urn(),
                bob.peer_id,
                bob.listen_addrs.clone(),
            )
            .await
            .expect("unable to clone project");
            state::get_project(&eve.peer, project.urn()).await?.unwrap()
        };

        let path = state::checkout(
            &bob.peer,
            project.urn(),
            alice.peer_id,
            bob.path.join("radicle"),
        )
        .await?;
        let commit_id = blocking({
            let project = project.clone();
            let bob_peer = bob.peer.clone();
            let bob_owner = bob.owner.clone();

            move || {
                let repo = git2::Repository::open(path)?;
                let oid = repo
                    .find_reference(&format!(
                        "refs/heads/{}",
                        project.subject().default_branch.clone().unwrap()
                    ))?
                    .target()
                    .expect("Missing first commit");
                let commit = repo.find_commit(oid)?;
                let commit_id = {
                    let empty_tree = {
                        let mut index = repo.index()?;
                        let oid = index.write_tree()?;
                        repo.find_tree(oid)?
                    };

                    let author = git2::Signature::now(
                        bob_owner.subject().name.as_str(),
                        &format!("{}@example.com", bob_owner.subject().name),
                    )?;
                    repo.commit(
                        Some(&format!(
                            "refs/heads/{}",
                            project.subject().default_branch.clone().unwrap()
                        )),
                        &author,
                        &author,
                        "Successor commit",
                        &empty_tree,
                        &[&commit],
                    )?
                };

                {
                    let mut rad =
                        Remote::rad_remote::<_, Fetchspec>(LocalUrl::from(project.urn()), None);
                    let _ = rad.push(
                        state::settings(&bob_peer),
                        &repo,
                        LocalPushspec::Matching {
                            pattern: RefLike::try_from(format!(
                                "refs/heads/{}",
                                project.subject().default_branch.clone().unwrap()
                            ))
                            .unwrap()
                            .into(),
                            force: Force::False,
                        },
                    )?;
                }

                Ok::<_, anyhow::Error>(commit_id)
            }
        })
        .await?;

        state::fetch(&eve.peer, project.urn(), bob.peer_id, bob.listen_addrs)
            .await
            .expect("fetch from bob");

        let path = state::checkout(
            &eve.peer,
            project.urn(),
            alice.peer_id,
            eve.path.join("radicle"),
        )
        .await?;

        blocking(move || {
            let repo = git2::Repository::open(path).unwrap();
            assert_matches!(repo.find_commit(commit_id), Err(_));
        })
        .await;

        Ok(())
    })
}

#[test]
fn track_peer() -> Result<(), anyhow::Error> {
    logging::init();

    let mut harness = Harness::new();
    let mut alice = harness.add_peer("alice", RunConfig::default(), &[])?;
    let bob = harness.add_peer(
        "bob",
        RunConfig::default(),
        &[Seed {
            addrs: alice.listen_addrs.clone(),
            peer_id: alice.peer_id,
        }],
    )?;
    harness.enter(async move {
        let project = state::init_project(
            &alice.peer,
            &alice.owner,
            shia_le_pathbuf(alice.path.join("radicle")),
        )
        .await?;

        state::clone_project(
            &bob.peer,
            project.urn(),
            alice.peer_id,
            alice.listen_addrs.clone(),
        )
        .await?;

        state::track(&alice.peer, project.urn(), bob.peer_id).await?;

        // FIXME: For some reason, we need to wait for two fetch events. Trying
        // to match on the project URN doesn't work.
        assert_fetched(&mut alice.events).await?;
        assert_fetched(&mut alice.events).await?;

        let tracked = state::tracked(&alice.peer, project.urn()).await?;
        assert!(tracked.iter().any(|peer| match peer {
            Peer::Remote { peer_id, status } =>
                *peer_id == bob.peer_id && matches!(status, peer::Status::Replicated(_)),
            _ => false,
        }));

        Ok(())
    })
}

#[test]
#[cfg(not(feature = "replication-v3"))]
fn replication_includes_user() -> Result<(), anyhow::Error> {
    use librad::{git, net};
    use tracing::{info, warn};

    logging::init();

    let mut harness = Harness::new();
    let alice = harness.add_peer("alice", RunConfig::default(), &[])?;
    let bob = harness.add_peer(
        "bob",
        RunConfig::default(),
        &[Seed {
            addrs: alice.listen_addrs.clone(),
            peer_id: alice.peer_id,
        }],
    )?;
    harness.enter(async move {
        let project = state::init_project(
            &alice.peer,
            &alice.owner,
            shia_le_pathbuf(alice.path.join("radicle")),
        )
        .await?;

        state::clone_project(
            &bob.peer,
            project.urn(),
            alice.peer_id,
            alice.listen_addrs.clone(),
        )
        .await?;

        state::track(&alice.peer, project.urn(), bob.peer_id).await?;
        // `state::track` already triggers a fetch by gossipping a `Have`, but
        // we don't know when it's done. A manual `state::fetch` is however
        // likely to be rejected due to the background one being in-flight. So
        // just try fetching until it succeeds, after which bobby should be on
        // alice's machine.
        loop {
            info!("fetching from bob to alice");
            let res = state::fetch(
                &alice.peer,
                project.urn(),
                bob.peer_id,
                bob.listen_addrs.clone(),
            )
            .await;
            match res {
                Err(state::Error::Replication(net::peer::error::Replicate::Replicate(
                    net::replication::error::Replicate::Retrying(
                        git::storage::fetcher::error::Retrying::Concurrent { .. },
                    ),
                ))) => {
                    warn!("unable to acquire fetch slot, trying again");
                    continue;
                },

                Ok(_) => break,
                Err(e) => return Err(e.into()),
            }
        }
        let bob_malkovich = state::get_user(&alice.peer, bob.owner.urn()).await?;

        assert_eq!(bob_malkovich, Some(bob.owner.into_inner().into_inner()));

        Ok(())
    })
}
