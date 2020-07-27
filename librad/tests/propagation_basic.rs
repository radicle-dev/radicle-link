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

    let peers = testnet::setup(2).await.unwrap();
    testnet::run_on_testnet(peers, async move |mut apis| {
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
                git2.clone_repo::<ProjectInfo>(radicle.urn().into_rad_url(peer1.peer_id().clone()))
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

    let peers = testnet::setup(2).await.unwrap();
    testnet::run_on_testnet(peers, async move |mut apis| {
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

        // Create project on peer1, and clone from peer2
        {
            peer1_storage.create_repo(&alice).unwrap();
            peer1_storage.create_repo(&radicle).unwrap();
            let peer2_storage = peer2_storage.reopen().unwrap();
            let url = radicle.urn().into_rad_url(peer1.peer_id().clone());
            tokio::task::spawn_blocking(move || {
                peer2_storage.clone_repo::<ProjectInfo>(url).unwrap();
            })
            .await
            .unwrap();
        }

        let global_settings = librad::git::local::transport::Settings {
            paths: peer1.paths().clone(),
            signer: peer1_key,
        }
        .global();

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
                .remote("origin", &LocalUrl::from(radicle.urn()).to_string())
                .unwrap();
            origin.push(&["refs/heads/master"], None).unwrap();

            commit_id
        };

        // Announce the update, and wait for peer2 to receive it
        {
            let peer2_events = peer2.subscribe().await;

            peer1
                .protocol()
                .announce(Gossip {
                    origin: peer1.peer_id().clone(),
                    urn: RadUrn {
                        path: uri::Path::parse("refs/heads/master").unwrap(),
                        ..radicle.urn()
                    },
                    rev: Some(Rev::Git(commit_id)),
                })
                .await;

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
