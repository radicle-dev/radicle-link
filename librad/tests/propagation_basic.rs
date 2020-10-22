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

#![feature(async_closure)]

use std::{convert::TryFrom, marker::PhantomData, time::Duration};

use futures::{future, stream::StreamExt};
use tempfile::tempdir;
use tokio::task::block_in_place;

use librad::{
    git::{
        ext,
        local::{self, transport::LocalTransportFactory, url::LocalUrl},
        storage,
        types::{remote::Remote, FlatRef, Force, NamespacedRef},
    },
    meta::{entity::Signatory, project::ProjectInfo},
    net::peer::{FetchInfo, Gossip, PeerEvent, Rev},
    signer::SomeSigner,
    uri::{self, RadUrn},
};

use librad_test::{
    git::initial_commit,
    logging,
    rad::{
        entity::{Alice, Radicle},
        testnet,
    },
};

#[tokio::test]
async fn can_clone() {
    logging::init();

    const NUM_PEERS: usize = 2;

    let peers = testnet::setup(NUM_PEERS).await.unwrap();
    testnet::run_on_testnet(peers, NUM_PEERS, async move |mut apis| {
        let (peer1, peer1_key) = apis.pop().unwrap();
        let (peer2, _) = apis.pop().unwrap();

        let mut alice = Alice::new(peer1_key.public());
        let mut radicle = Radicle::new(&alice);
        {
            let resolves_to_alice = alice.clone();
            alice
                .sign(&peer1_key, &Signatory::OwnedKey, &resolves_to_alice)
                .unwrap();
            radicle
                .sign(
                    &peer1_key,
                    &Signatory::User(alice.urn()),
                    &resolves_to_alice,
                )
                .unwrap();
        }

        let radicle_urn = radicle.urn();

        {
            let alice = alice.clone();
            peer1
                .with_storage(move |storage| {
                    storage.create_repo(&alice).unwrap();
                    storage.create_repo(&radicle).unwrap();
                })
                .await
                .unwrap();
        }

        peer2
            .with_storage(move |storage| {
                let peer1_id = peer1.peer_id();
                storage
                    .clone_repo::<ProjectInfo, _>(radicle_urn.clone().into_rad_url(peer1_id), None)
                    .unwrap();
                // sanity check
                storage.open_repo(radicle_urn.clone()).unwrap();

                // check rad/self of peer1 exists
                storage.get_rad_self_of(&radicle_urn, peer1_id).unwrap();

                // check user metadata exists
                storage.some_metadata_of(&alice.urn(), None).unwrap();
            })
            .await
            .unwrap();
    })
    .await;
}

#[tokio::test]
async fn can_clone_disconnected() {
    logging::init();

    const NUM_PEERS: usize = 2;

    let peers = testnet::setup_disconnected(NUM_PEERS).await.unwrap();
    testnet::run_on_testnet(peers, 0, async move |mut apis| {
        let (peer1, peer1_key) = apis.pop().unwrap();
        let (peer2, _) = apis.pop().unwrap();

        let mut alice = Alice::new(peer1_key.public());
        let mut radicle = Radicle::new(&alice);
        {
            let resolves_to_alice = alice.clone();
            alice
                .sign(&peer1_key, &Signatory::OwnedKey, &resolves_to_alice)
                .unwrap();
            radicle
                .sign(
                    &peer1_key,
                    &Signatory::User(alice.urn()),
                    &resolves_to_alice,
                )
                .unwrap();
        }

        let radicle_urn = radicle.urn();

        peer1
            .with_storage(move |storage| {
                storage.create_repo(&alice).unwrap();
                storage.create_repo(&radicle).unwrap();
            })
            .await
            .unwrap();
        peer2
            .with_storage(move |storage| {
                storage
                    .clone_repo::<ProjectInfo, _>(
                        radicle_urn.clone().into_rad_url(peer1.peer_id()),
                        Some(peer1.listen_addr()),
                    )
                    .unwrap();
                // sanity check
                storage.open_repo(radicle_urn).unwrap();
            })
            .await
            .unwrap();
    })
    .await;
}

#[tokio::test(core_threads = 2)]
async fn fetches_on_gossip_notify() {
    logging::init();

    const NUM_PEERS: usize = 2;

    let peers = testnet::setup(NUM_PEERS).await.unwrap();
    testnet::run_on_testnet(peers, NUM_PEERS, async move |mut apis| {
        let (peer1, peer1_key) = apis.pop().unwrap();
        let (peer2, _) = apis.pop().unwrap();

        let mut alice = Alice::new(peer1_key.public());
        let mut radicle = Radicle::new(&alice);
        {
            let resolves_to_alice = alice.clone();
            alice
                .sign(&peer1_key, &Signatory::OwnedKey, &resolves_to_alice)
                .unwrap();
            radicle
                .sign(
                    &peer1_key,
                    &Signatory::User(alice.urn()),
                    &resolves_to_alice,
                )
                .unwrap();
        }

        let peer2_events = peer2.subscribe().await;
        let urn = radicle.urn();
        let alice_name = alice.name();

        // Create project on peer1, and clone from peer2
        {
            let alice = alice.clone();
            let radicle = radicle.clone();
            peer1
                .with_storage(move |storage| {
                    storage.create_repo(&alice).unwrap();
                    storage.create_repo(&radicle).unwrap();
                })
                .await
                .unwrap();
        }

        {
            let radicle_at_peer1 = radicle.urn().into_rad_url(peer1.peer_id());
            peer2
                .with_storage(move |storage| {
                    storage
                        .clone_repo::<ProjectInfo, _>(radicle_at_peer1, None)
                        .unwrap();
                })
                .await
                .unwrap();
        }

        // Check out a working copy on peer1, add a commit, and push it
        let commit_id = block_in_place(|| {
            librad::git::local::transport::register();
            let transport_results = LocalTransportFactory::configure(local::transport::Settings {
                paths: peer1.paths().clone(),
                signer: SomeSigner { signer: peer1_key }.into(),
            });

            let tmp = tempdir().unwrap();
            let repo = git2::Repository::init(tmp.path()).unwrap();

            let mut remote_callbacks = git2::RemoteCallbacks::new();
            remote_callbacks.push_update_reference(|refname, maybe_error| match maybe_error {
                None => {
                    let rev = repo.find_reference(refname)?.target().unwrap();

                    futures::executor::block_on(peer1.protocol().announce(Gossip {
                        origin: Some(peer1.peer_id()),
                        urn: RadUrn {
                            path: uri::Path::parse(refname).unwrap(),
                            ..radicle.urn()
                        },
                        rev: Some(Rev::Git(rev)),
                    }));

                    Ok(())
                },

                Some(err) => Err(git2::Error::from_str(&format!(
                    "Remote rejected {}: {}",
                    refname, err
                ))),
            });

            let url = LocalUrl::from_urn(urn.clone(), peer1.peer_id());

            let heads = NamespacedRef::heads(urn.clone().id, Some(peer1.peer_id()));
            let remotes = FlatRef::heads(
                PhantomData,
                ext::RefLike::try_from(format!("{}@{}", alice_name, peer1.peer_id())).unwrap(),
            );

            let remote = Remote::rad_remote(url, Some(remotes.refspec(heads, Force::True).boxed()));

            let oid =
                initial_commit(&repo, remote, "refs/heads/master", Some(remote_callbacks)).unwrap();
            assert!(transport_results.wait(Duration::from_secs(3)).is_some());

            oid
        });

        // Wait for peer2 to receive the gossip announcement
        {
            let peer1_id = peer1.peer_id();
            tokio::time::timeout(
                Duration::from_secs(5),
                peer2_events
                    .filter(|event| match event {
                        PeerEvent::GossipFetch(FetchInfo { provider, .. }) => {
                            future::ready(*provider == peer1_id)
                        },
                    })
                    .map(|_| ())
                    .next(),
            )
            .await
            .unwrap();
        }

        // Check that peer2 has fetched the update
        let peer2_has_commit = peer2
            .with_storage(move |storage| {
                storage.has_commit(
                    &RadUrn {
                        path: uri::Path::parse(format!(
                            "refs/remotes/{}/heads/master",
                            peer1.peer_id()
                        ))
                        .unwrap(),
                        ..radicle.urn()
                    },
                    commit_id,
                )
            })
            .await
            .unwrap()
            .unwrap();
        assert!(peer2_has_commit);
    })
    .await;
}

#[tokio::test]
async fn all_metadata_returns_only_local_projects() {
    logging::init();

    const NUM_PEERS: usize = 3;

    let peers = testnet::setup(NUM_PEERS).await.unwrap();
    testnet::run_on_testnet(peers, NUM_PEERS, async move |mut apis| {
        let (peer1, peer1_key) = apis.pop().unwrap();
        let (peer2, _) = apis.pop().unwrap();
        let (peer3, _) = apis.pop().unwrap();

        let mut alice = Alice::new(peer1_key.public());
        let mut radicle = Radicle::new(&alice);
        {
            let resolves_to_alice = alice.clone();
            alice
                .sign(&peer1_key, &Signatory::OwnedKey, &resolves_to_alice)
                .unwrap();
            radicle
                .sign(
                    &peer1_key,
                    &Signatory::User(alice.urn()),
                    &resolves_to_alice,
                )
                .unwrap();
        }

        let radicle_at_peer1 = radicle.urn().into_rad_url(peer1.peer_id());
        let radicle_at_peer2 = radicle.urn().into_rad_url(peer2.peer_id());

        peer1
            .with_storage(move |storage| {
                storage.create_repo(&alice).unwrap();
                storage.create_repo(&radicle).unwrap();
            })
            .await
            .unwrap();
        peer2
            .with_storage(move |storage| {
                storage
                    .clone_repo::<ProjectInfo, _>(radicle_at_peer1, None)
                    .unwrap();
            })
            .await
            .unwrap();
        let all_metadata_acc_to_peer3 = peer3
            .with_storage(move |storage| {
                storage.clone_repo::<ProjectInfo, _>(radicle_at_peer2, None)?;
                Ok::<_, storage::Error>(storage.all_metadata()?.collect::<Vec<_>>())
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(2, all_metadata_acc_to_peer3.len());
    })
    .await;
}

/// Given that a) a peer 1 holds a given URN and b) that same peer is a seed of
/// a peer 2, verify that requesting peer 2 for providers for said URN returns
/// peer 1.
///
/// Following that, verify that cloning from the returned PeerId means we have
/// the URN in our monorepo.
#[tokio::test]
async fn ask_and_clone() {
    logging::init();
    const NUM_PEERS: usize = 2;
    let peers = testnet::setup(NUM_PEERS).await.unwrap();

    testnet::run_on_testnet(peers, NUM_PEERS, async move |mut apis| {
        let (peer1, peer1_key) = apis.pop().unwrap();
        let mut alice = Alice::new(peer1_key.public());
        let mut radicle = Radicle::new(&alice);

        {
            let resolves_to_alice = alice.clone();
            alice
                .sign(&peer1_key, &Signatory::OwnedKey, &resolves_to_alice)
                .unwrap();
            radicle
                .sign(
                    &peer1_key,
                    &Signatory::User(alice.urn()),
                    &resolves_to_alice,
                )
                .unwrap();
        }

        let repo_urn = peer1
            .with_storage(move |storage| {
                storage.create_repo(&alice).unwrap();
                storage.create_repo(&radicle).unwrap().urn
            })
            .await
            .unwrap();

        let (peer2, _) = apis.pop().unwrap();
        let res = peer2
            .providers(repo_urn.clone(), Duration::from_secs(5))
            .await
            .next()
            .await;

        let peer_id = match res {
            Some(peer_info) => peer_info.peer_id,
            None => panic!("Expected to have obtained peer1 but got None instead"),
        };

        let peer2_has_urn = {
            let repo_urn = repo_urn.clone();
            peer2
                .with_storage(move |storage| storage.has_urn(&repo_urn))
                .await
                .unwrap()
                .unwrap()
        };
        assert_eq!(
            false, peer2_has_urn,
            "expected peer2 to not have URN {} yet",
            repo_urn
        );

        {
            let url = repo_urn.clone().into_rad_url(peer_id);
            peer2
                .with_storage(move |storage| {
                    storage.clone_repo::<ProjectInfo, _>(url, None).unwrap();
                })
                .await
                .unwrap();
        }

        let peer2_has_urn = {
            let repo_urn = repo_urn.clone();
            peer2
                .with_storage(move |storage| storage.has_urn(&repo_urn))
                .await
                .unwrap()
                .unwrap()
        };
        assert_eq!(
            true, peer2_has_urn,
            "expected peer2 to have URN {}",
            repo_urn
        )
    })
    .await;
}

#[tokio::test(core_threads = 2)]
async fn menage_a_troi() {
    logging::init();

    const NUM_PEERS: usize = 3;

    let peers = testnet::setup(NUM_PEERS).await.unwrap();
    testnet::run_on_testnet(peers, NUM_PEERS, async move |mut apis| {
        let (peer1, peer1_key) = apis.pop().unwrap();
        let peer1_id = peer1.peer_id();
        let peer1_addr = peer1.listen_addr();

        let (peer2, _) = apis.pop().unwrap();
        let peer2_id = peer2.peer_id();
        let peer2_addr = peer2.listen_addr();

        let (peer3, _) = apis.pop().unwrap();

        let mut alice = Alice::new(peer1_key.public());
        let mut radicle = Radicle::new(&alice);
        {
            let resolves_to_alice = alice.clone();
            alice
                .sign(&peer1_key, &Signatory::OwnedKey, &resolves_to_alice)
                .unwrap();
            radicle
                .sign(
                    &peer1_key,
                    &Signatory::User(alice.urn()),
                    &resolves_to_alice,
                )
                .unwrap();
        }

        let urn = radicle.urn();
        let default_branch = radicle.default_branch().to_string();
        let alice_name = alice.name().to_string();

        peer1
            .with_storage(move |storage| {
                storage.create_repo(&alice).unwrap();
                storage.create_repo(&radicle).unwrap();
            })
            .await
            .unwrap();

        let tmp = tempfile::tempdir().unwrap();
        block_in_place(|| {
            librad::git::local::transport::register();
            let transport_results = LocalTransportFactory::configure(local::transport::Settings {
                paths: peer1.paths().clone(),
                signer: SomeSigner { signer: peer1_key }.into(),
            });

            // Perform commit and push to working copy on peer1
            let repo = git2::Repository::init(tmp.path().join("peer1")).unwrap();
            let url = LocalUrl::from_urn(urn.clone(), peer1_id);

            let heads = NamespacedRef::heads(urn.clone().id, Some(peer1_id));
            let remotes = FlatRef::heads(
                PhantomData,
                ext::RefLike::try_from(format!("{}@{}", alice_name, peer1_id)).unwrap(),
            );

            let remote = Remote::rad_remote(url, Some(remotes.refspec(heads, Force::True).boxed()));

            initial_commit(
                &repo,
                remote,
                &format!("refs/heads/{}", default_branch),
                None,
            )
            .unwrap();
            assert!(transport_results.wait(Duration::from_secs(3)).is_some());
        });

        let head = NamespacedRef::head(
            urn.clone().id,
            peer1_id,
            ext::RefLike::try_from(default_branch.as_str()).unwrap(),
        );
        let peer2_has_ref = {
            let head = head.clone();
            let url = urn.clone().into_rad_url(peer1_id);
            peer2
                .with_storage(move |storage| {
                    storage
                        .clone_repo::<ProjectInfo, _>(url, Some(peer1_addr))
                        .unwrap();
                    storage.has_ref(&head).unwrap()
                })
                .await
                .unwrap()
        };
        let peer3_has_ref = {
            let head = head.clone();
            let url = urn.clone().into_rad_url(peer2_id);
            peer3
                .with_storage(move |storage| {
                    storage
                        .clone_repo::<ProjectInfo, _>(url, Some(peer2_addr))
                        .unwrap();
                    storage.has_ref(&head).unwrap()
                })
                .await
                .unwrap()
        };

        assert!(peer2_has_ref, format!("peer 2 missing ref '{}'", head));
        assert!(peer3_has_ref, format!("peer 3 missing ref '{}'", head));
    })
    .await;
}
