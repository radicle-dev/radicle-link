// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom, ops::Index, time::Duration};

use assert_matches::assert_matches;
use futures::{future, StreamExt as _};
use pretty_assertions::assert_eq;
use tempfile::tempdir;
use tokio::time::timeout;

use librad::{
    git::{
        local::url::LocalUrl,
        types::{remote::LocalPushspec, Fetchspec, Force, Remote},
    },
    reflike,
};
use radicle_git_ext::RefLike;

use radicle_daemon::{
    identities::payload::Person,
    project::{peer, Peer},
    state,
    state::init_owner,
    RunConfig,
};

use crate::{assert_event, daemon::common::*, logging, rad::testnet};

fn config() -> testnet::Config {
    testnet::Config {
        num_peers: nonzero!(2usize),
        min_connected: 2,
        bootstrap: testnet::Bootstrap::from_env(),
    }
}

#[test]
fn can_clone_project() {
    logging::init();

    let net = testnet::run(config()).unwrap();
    net.enter(async {
        let peer1 = net.peers().index(0);
        let peer2 = net.peers().index(1);

        let tmp = tempdir().unwrap();
        let owner = state::init_owner(
            &peer1,
            Person {
                name: "alice".into(),
            },
        )
        .await
        .unwrap();
        let _bob = state::init_owner(&peer2, Person { name: "bob".into() })
            .await
            .unwrap();

        let project =
            state::init_project(&peer1, &owner, shia_le_pathbuf(tmp.path().to_path_buf()))
                .await
                .unwrap();

        {
            let remote = peer1.peer_id();
            let addrs = peer1.listen_addrs().to_vec().into_iter();
            state::clone_project(&peer2, project.urn(), remote, addrs, None)
                .await
                .unwrap();
        }

        let have = state::list_projects(&peer2)
            .await
            .unwrap()
            .into_iter()
            .map(|p| p.urn())
            .collect::<Vec<_>>();
        let want = vec![project.urn()];
        assert_eq!(have, want);

        let another_peer = librad::peer::PeerId::from(librad::keys::SecretKey::new());
        state::track(&peer2, project.urn(), another_peer)
            .await
            .unwrap();
        let mut have = state::tracked(&peer2, project.urn())
            .await
            .unwrap()
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
                peer_id: peer1.peer_id(),
                status: peer::Status::replicated(
                    peer::Role::Maintainer,
                    owner.subject().name.to_string(),
                ),
            },
        ];
        assert_eq!(have, want);
    })
}

#[test]
fn can_clone_user() {
    logging::init();

    let net = testnet::run(config()).unwrap();
    net.enter(async {
        let peer1 = net.peers().index(0);
        let peer2 = net.peers().index(1);
        let alice = init_owner(
            &peer1,
            Person {
                name: "alice".into(),
            },
        )
        .await
        .unwrap();
        let _bob = init_owner(&peer2, Person { name: "bob".into() });
        {
            let remote = peer1.peer_id();
            let addrs = peer1.listen_addrs().to_vec().into_iter();
            state::clone_user(&peer2, alice.urn(), remote, addrs, None)
                .await
                .unwrap();
        }
        let want = state::list_users(&peer2)
            .await
            .unwrap()
            .into_iter()
            .map(|user| user.urn())
            .collect::<Vec<_>>();
        let have = vec![alice.urn()];

        assert_eq!(want, have);
    })
}

#[test]
fn can_fetch_project_changes() {
    logging::init();

    logging::init();

    let net = testnet::run(config()).unwrap();
    net.enter(async {
        let peer1 = net.peers().index(0);
        let peer2 = net.peers().index(1);

        let alice = init_owner(
            &peer1,
            Person {
                name: "alice".into(),
            },
        )
        .await
        .unwrap();
        let tmp = tempdir().unwrap();
        let alice_repo_path = tmp.path().to_path_buf();

        let _bob = init_owner(&peer2, Person { name: "bob".into() })
            .await
            .unwrap();

        let project = state::init_project(&peer1, &alice, shia_le_pathbuf(alice_repo_path.clone()))
            .await
            .unwrap();

        {
            let remote = peer1.peer_id();
            let addrs = peer1.listen_addrs().to_vec();
            state::clone_project(&peer2, project.urn(), remote, addrs, None)
                .await
                .expect("unable to clone project")
        };

        assert_eq!(
            state::list_projects(&peer2)
                .await
                .unwrap()
                .into_iter()
                .map(|project| project.urn())
                .collect::<Vec<_>>(),
            vec![project.urn()]
        );

        let commit_id = {
            let repo =
                git2::Repository::open(alice_repo_path.join(project.subject().name.to_string()))
                    .unwrap();
            let oid = repo
                .find_reference(&format!(
                    "refs/heads/{}",
                    project.subject().default_branch.clone().unwrap()
                ))
                .unwrap()
                .target()
                .expect("Missing first commit");
            let commit = repo.find_commit(oid).unwrap();
            let commit_id = {
                let empty_tree = {
                    let mut index = repo.index().unwrap();
                    let oid = index.write_tree().unwrap();
                    repo.find_tree(oid).unwrap()
                };

                let author =
                    git2::Signature::now(&alice.subject().name.to_string(), "alice@example.com")
                        .unwrap();
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
                )
                .unwrap()
            };

            {
                let mut rad = Remote::<LocalUrl>::rad_remote::<_, Fetchspec>(
                    LocalUrl::from(project.urn()),
                    None,
                );
                let branch =
                    RefLike::try_from(project.subject().default_branch.as_ref().unwrap().as_str())
                        .unwrap();
                let _ = rad
                    .push(
                        state::settings(&peer1),
                        &repo,
                        LocalPushspec::Matching {
                            pattern: reflike!("refs/heads").join(branch).into(),
                            force: Force::False,
                        },
                    )
                    .unwrap();
            }

            commit_id
        };

        {
            let remote = peer1.peer_id();
            let addrs = peer1.listen_addrs().to_vec();
            state::fetch(&peer2, project.urn(), remote, addrs, None)
                .await
                .unwrap();
        };

        let remote = peer1.peer_id();
        let has_commit = state::has_commit(
            &peer2,
            project.urn().with_path(Some(
                RefLike::try_from(format!(
                    "refs/remotes/{}/heads/{}",
                    remote,
                    project.subject().default_branch.clone().unwrap(),
                ))
                .unwrap(),
            )),
            radicle_daemon::git_ext::Oid::from(commit_id),
        )
        .await
        .unwrap();
        assert!(has_commit);
    })
}

#[test]
fn can_sync_on_startup() {
    logging::init();

    let net = testnet::run(config()).unwrap();
    net.enter(async {
        let peer1 = net.peers().index(0);
        let peer2 = net.peers().index(1);

        let tmp = tempdir().unwrap();
        let config = RunConfig {
            sync: radicle_daemon::peer::run_config::Sync {
                interval: Duration::from_millis(100),
            },
            ..RunConfig::default()
        };
        let owner = state::init_owner(
            &peer1,
            Person {
                name: "alice".into(),
            },
        )
        .await
        .unwrap();
        let store = kv::Store::new(kv::Config::new(tmp.path().join("store"))).unwrap();
        let disco = radicle_daemon::config::static_seed_discovery(&[radicle_daemon::seed::Seed {
            peer_id: peer2.peer_id(),
            addrs: peer2.listen_addrs().to_vec(),
        }]);
        let alice_daemon = radicle_daemon::Peer::with_peer((*peer1).clone(), disco, store, config);
        let mut alice_events = alice_daemon.subscribe();
        tokio::task::spawn(alice_daemon.run());

        let store = kv::Store::new(kv::Config::new(tmp.path().join("bob_store"))).unwrap();
        let disco = radicle_daemon::config::static_seed_discovery(&[radicle_daemon::seed::Seed {
            peer_id: peer1.peer_id(),
            addrs: peer1.listen_addrs().to_vec(),
        }]);
        let bob_daemon =
            radicle_daemon::Peer::with_peer((*peer1).clone(), disco, store, RunConfig::default());
        let bob_events = bob_daemon.subscribe();
        tokio::task::spawn(bob_daemon.run());

        let _bob = state::init_owner(&peer2, Person { name: "bob".into() })
            .await
            .unwrap();
        connected(bob_events, 1).await.unwrap();

        let _project =
            state::init_project(&peer1, &owner, shia_le_pathbuf(tmp.path().to_path_buf()))
                .await
                .unwrap();

        let remote = peer2.peer_id();
        assert_event!(
            alice_events,
            radicle_daemon::PeerEvent::PeerSynced(peer_id) if peer_id == remote
        )
        .unwrap();
    })
}

#[test]
fn can_create_working_copy_of_peer() {
    let config = testnet::Config {
        num_peers: nonzero!(3usize),
        min_connected: 3,
        bootstrap: testnet::Bootstrap::from_env(),
    };
    let net = testnet::run(config).unwrap();
    net.enter(async {
        let peer1 = net.peers().index(0);
        let peer2 = net.peers().index(1);
        let peer3 = net.peers().index(2);

        let tmp = tempdir().unwrap();
        let owner = state::init_owner(
            &peer1,
            Person {
                name: "alice".into(),
            },
        )
        .await
        .unwrap();

        let alice_repo_path = tmp.path().join("alice");
        let bob = state::init_owner(&peer2, Person { name: "bob".into() })
            .await
            .unwrap();
        let bob_repo_path = tmp.path().join("bob");

        let _eve = state::init_owner(&peer3, Person { name: "eve".into() })
            .await
            .unwrap();
        let eve_repo_path = tmp.path().join("eve");

        let project = state::init_project(&peer1, &owner, shia_le_pathbuf(alice_repo_path))
            .await
            .unwrap();

        let project = {
            let alice = peer1.peer_id();
            let addrs = peer1.listen_addrs().to_vec();
            state::clone_project(&peer2, project.urn(), alice, addrs, None)
                .await
                .unwrap();

            let bob = peer2.peer_id();
            let addrs = peer2.listen_addrs().to_vec();
            state::clone_project(&peer3, project.urn(), bob, addrs, None)
                .await
                .unwrap();
            state::get_project(&peer3, project.urn())
                .await
                .unwrap()
                .unwrap()
        };

        let commit_id = {
            let peer1_id = peer1.peer_id();
            let path = state::checkout(&peer2, project.urn(), peer1_id, bob_repo_path)
                .await
                .unwrap();

            let repo = git2::Repository::open(path).unwrap();
            let oid = repo
                .find_reference(&format!(
                    "refs/heads/{}",
                    project.subject().default_branch.clone().unwrap()
                ))
                .unwrap()
                .target()
                .expect("Missing first commit");
            let commit = repo.find_commit(oid).unwrap();
            let commit_id = {
                let empty_tree = {
                    let mut index = repo.index().unwrap();
                    let oid = index.write_tree().unwrap();
                    repo.find_tree(oid).unwrap()
                };

                let author = git2::Signature::now(
                    bob.subject().name.as_str(),
                    &format!("{}@example.com", bob.subject().name),
                )
                .unwrap();
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
                )
                .unwrap()
            };

            {
                let mut rad =
                    Remote::rad_remote::<_, Fetchspec>(LocalUrl::from(project.urn()), None);
                let _ = rad
                    .push(
                        state::settings(&peer2),
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
                    )
                    .unwrap();
            }

            commit_id
        };

        {
            let remote = peer2.peer_id();
            let addrs = peer2.listen_addrs().to_vec();
            state::fetch(&peer3, project.urn(), remote, addrs, None)
                .await
                .unwrap();
        }

        let path = {
            let peer1_id = peer1.peer_id();
            state::checkout(&peer3, project.urn(), peer1_id, eve_repo_path)
                .await
                .unwrap()
        };

        let repo = git2::Repository::open(path).unwrap();
        assert_matches!(repo.find_commit(commit_id), Err(_));
    })
}

#[test]
fn track_peer() {
    let net = testnet::run(config()).unwrap();
    net.enter(async {
        let peer1 = net.peers().index(0);
        let peer2 = net.peers().index(1);

        let tmp = tempdir().unwrap();
        let owner = state::init_owner(
            &peer1,
            Person {
                name: "alice".into(),
            },
        )
        .await
        .unwrap();

        let alice_repo_path = tmp.path().join("alice");
        let store = kv::Store::new(kv::Config::new(tmp.path().join("store"))).unwrap();
        let disco = radicle_daemon::config::static_seed_discovery(&[radicle_daemon::seed::Seed {
            peer_id: peer2.peer_id(),
            addrs: peer2.listen_addrs().to_vec(),
        }]);
        let alice_daemon =
            radicle_daemon::Peer::with_peer((*peer1).clone(), disco, store, RunConfig::default());
        let mut alice_events = alice_daemon.subscribe();
        tokio::task::spawn(alice_daemon.run());

        let _bob = state::init_owner(&peer2, Person { name: "bob".into() })
            .await
            .unwrap();

        let project = state::init_project(&peer1, &owner, shia_le_pathbuf(alice_repo_path))
            .await
            .unwrap();

        state::clone_project(
            &peer2,
            project.urn(),
            peer1.peer_id(),
            peer1.listen_addrs().to_vec(),
            None,
        )
        .await
        .unwrap();

        state::track(&peer1, project.urn(), peer2.peer_id())
            .await
            .unwrap();

        assert_event!(
            alice_events,
            radicle_daemon::PeerEvent::GossipFetched { .. }
        )
        .unwrap();

        let tracked = state::tracked(&peer1, project.urn()).await.unwrap();
        assert!(tracked.iter().any(|peer| match peer {
            Peer::Remote { peer_id, status } =>
                *peer_id == peer2.peer_id() && matches!(status, peer::Status::Replicated(_)),
            _ => false,
        }));
    })
}
