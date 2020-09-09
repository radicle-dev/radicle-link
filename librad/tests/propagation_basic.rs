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

use std::time::Duration;

use futures::{future, stream::StreamExt};
use tempfile::tempdir;

use librad::{
    git::local::url::LocalUrl,
    meta::{entity::Signatory, project::ProjectInfo},
    net::peer::{FetchInfo, Gossip, PeerEvent, Rev},
    signer::SomeSigner,
    uri::{self, RadUrn},
};

use librad_test::{
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

        tokio::task::spawn_blocking(move || {
            peer1.storage().create_repo(&alice).unwrap();
            peer1.storage().create_repo(&radicle).unwrap();
            {
                let git2 = peer2.storage();
                git2.clone_repo::<ProjectInfo, _>(
                    radicle.urn().into_rad_url(peer1.peer_id().clone()),
                    None,
                )
                .unwrap();
                // sanity check
                git2.open_repo(radicle.urn()).unwrap();
            }
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

        tokio::task::spawn_blocking(move || {
            peer1.storage().create_repo(&alice).unwrap();
            peer1.storage().create_repo(&radicle).unwrap();
            {
                let git2 = peer2.storage();
                git2.clone_repo::<ProjectInfo, _>(
                    radicle.urn().into_rad_url(peer1.peer_id().clone()),
                    Some(peer1.listen_addr()),
                )
                .unwrap();
                // sanity check
                git2.open_repo(radicle.urn()).unwrap();
            }
        })
        .await
        .unwrap();
    })
    .await;
}

#[tokio::test]
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

        let peer1_storage = peer1.storage();
        let peer2_storage = peer2.storage();
        let peer2_events = peer2.subscribe().await;

        // Create project on peer1, and clone from peer2
        {
            peer1_storage.create_repo(&alice).unwrap();
            peer1_storage.create_repo(&radicle).unwrap();
            let peer2_storage = peer2_storage.reopen().unwrap();
            let url = radicle.urn().into_rad_url(peer1.peer_id().clone());
            tokio::task::spawn_blocking(move || {
                peer2_storage
                    .clone_repo::<ProjectInfo, _>(url, None)
                    .unwrap();
            })
            .await
            .unwrap();
        }

        let global_settings = librad::git::local::transport::Settings {
            paths: peer1.paths().clone(),
            signer: SomeSigner { signer: peer1_key }.into(),
        };

        // Check out a working copy on peer1, add a commit, and push it
        let commit_id = {
            librad::git::local::transport::register(global_settings);

            let tmp = tempdir().unwrap();
            let repo = git2::Repository::init(tmp.path()).unwrap();
            let commit_id = {
                let empty_tree = {
                    let mut index = repo.index().unwrap();
                    let oid = index.write_tree().unwrap();
                    repo.find_tree(oid).unwrap()
                };
                let author = git2::Signature::now("The Animal", "animal@muppets.com").unwrap();
                repo.commit(
                    Some("refs/heads/master"),
                    &author,
                    &author,
                    "Initial commit",
                    &empty_tree,
                    &[],
                )
                .unwrap()
            };
            let mut origin = repo
                .remote(
                    "origin",
                    &LocalUrl::from_urn(radicle.urn(), peer1.peer_id().clone()).to_string(),
                )
                .unwrap();

            let mut remote_callbacks = git2::RemoteCallbacks::new();
            remote_callbacks.push_update_reference(|refname, maybe_error| match maybe_error {
                None => {
                    let rev = repo.find_reference(refname)?.target().unwrap();

                    futures::executor::block_on(peer1.protocol().announce(Gossip {
                        origin: Some(peer1.peer_id().clone()),
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

            origin
                .push(
                    &["refs/heads/master"],
                    Some(git2::PushOptions::new().remote_callbacks(remote_callbacks)),
                )
                .unwrap();

            commit_id
        };

        // Wait for peer2 to receive the gossip announcement
        {
            let peer1_id = peer1.peer_id();
            tokio::time::timeout(
                Duration::from_secs(5),
                peer2_events
                    .filter(|event| match event {
                        PeerEvent::GossipFetch(FetchInfo { provider, .. }) => {
                            future::ready(provider == peer1_id)
                        },
                    })
                    .map(|_| ())
                    .next(),
            )
            .await
            .unwrap();
        }

        // Check that peer2 has fetched the update
        assert!(peer2_storage
            .has_commit(
                &RadUrn {
                    path: uri::Path::parse(format!(
                        "refs/remotes/{}/heads/master",
                        peer1.peer_id()
                    ))
                    .unwrap(),
                    ..radicle.urn()
                },
                commit_id
            )
            .unwrap());
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

        tokio::task::spawn_blocking(move || {
            peer1.storage().create_repo(&alice).unwrap();
            peer1.storage().create_repo(&radicle).unwrap();
            let git2 = peer2.storage();
            git2.clone_repo::<ProjectInfo, _>(
                radicle.urn().into_rad_url(peer1.peer_id().clone()),
                None,
            )
            .unwrap();
            let git3 = peer3.storage();
            git3.clone_repo::<ProjectInfo, _>(
                radicle.urn().into_rad_url(peer2.peer_id().clone()),
                None,
            )
            .unwrap();
            let metadata_vec: Vec<_> = git3.all_metadata().unwrap().collect();
            assert_eq!(1, metadata_vec.len());
        })
        .await
        .unwrap();
    })
    .await;
}

/// Given that a) a peer 1 holds a given URN and b) that same peer is a seed of
/// a peer 2, verify that requesting peer 2 for providers for said URN returns
/// peer 1.
#[tokio::test]
async fn providers_works() {
    use std::time::Duration;
    use tokio::time::timeout;

    logging::init();
    const NUM_PEERS: usize = 2;
    let peers = testnet::setup(NUM_PEERS).await.unwrap();

    testnet::run_on_testnet(peers, NUM_PEERS, async move |mut apis| {
        let (peer1, peer1_key) = apis.pop().unwrap();
        let mut alice = Alice::new(peer1_key.public());
        let mut radicle = Radicle::new(&alice);
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

        let peer1_id = peer1.peer_id().clone();
        let repo_urn = tokio::task::spawn_blocking(move || {
            let git = peer1.storage();
            git.create_repo(&alice).unwrap();
            git.create_repo(&radicle).unwrap().urn
        })
        .await
        .unwrap();

        let (peer2, _) = apis.pop().unwrap();
        let res = timeout(
            Duration::from_secs(5),
            peer2.providers(repo_urn).await.next(),
        )
        .await;

        match res {
            Ok(Some(peer_info)) => assert_eq!(
                peer_info.peer_id, peer1_id,
                "Expected it to be {} but got {} instead",
                peer1_id, peer_info.peer_id
            ),
            Ok(None) => {
                panic!("Expected to have obtained the peer1 but got None instead");
            },
            Err(e) => {
                panic!("Didn't find any peer before the timeout: {}", e);
            },
        }
    })
    .await;
}
