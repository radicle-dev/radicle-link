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

use assert_matches::assert_matches;
use futures::{future, stream::StreamExt};
use tempfile::tempdir;
use tokio::task::block_in_place;

use librad::{
    git::{
        identities::{self, SomeIdentity},
        local::{self, transport::LocalTransportFactory, url::LocalUrl},
        replication,
        types::{namespace::Namespace, remote::Remote, FlatRef, Force, NamespacedRef},
    },
    git_ext as ext,
    net::peer::{FetchInfo, Gossip, PeerEvent, Rev},
    reflike,
    signer::SomeSigner,
};

use librad_test::{
    git::initial_commit,
    logging,
    rad::{
        identities::{create_test_project, TestProject},
        testnet,
    },
};

#[tokio::test]
async fn can_clone() {
    logging::init();

    const NUM_PEERS: usize = 2;

    let peers = testnet::setup(NUM_PEERS).await.unwrap();
    testnet::run_on_testnet(peers, NUM_PEERS, async move |mut apis| {
        let (peer1, _) = apis.pop().unwrap();
        let (peer2, _) = apis.pop().unwrap();

        let TestProject { project, owner } = peer1
            .with_storage(move |storage| create_test_project(&storage))
            .await
            .unwrap()
            .unwrap();

        peer2
            .with_storage(move |storage| {
                let urn = project.urn();
                replication::replicate(&storage, None, urn.clone(), peer1.peer_id(), None).unwrap();

                // check rad/self of peer1 exists
                assert!(
                    storage
                        .has_ref(&NamespacedRef::rad_self(
                            Namespace::from(&urn),
                            peer1.peer_id()
                        ))
                        .unwrap(),
                    "`refs/remotes/<peer1>/rad/self` should exist"
                );

                // check we have a top-level namespace for `owner`
                let urn = owner.urn();
                assert_eq!(
                    Some(owner),
                    identities::user::get(&storage, &urn).unwrap(),
                    "alice should be a first class citizen"
                )
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
        let (peer1, _) = apis.pop().unwrap();
        let (peer2, _) = apis.pop().unwrap();

        let TestProject { project, owner } = peer1
            .with_storage(move |storage| create_test_project(&storage))
            .await
            .unwrap()
            .unwrap();

        peer2
            .with_storage(move |storage| {
                let urn = project.urn();
                replication::replicate(
                    &storage,
                    None,
                    urn.clone(),
                    peer1.peer_id(),
                    Some(peer1.listen_addr()),
                )
                .unwrap();

                // check rad/self of peer1 exists
                assert!(
                    storage
                        .has_ref(&NamespacedRef::rad_self(
                            Namespace::from(&urn),
                            peer1.peer_id()
                        ))
                        .unwrap(),
                    "`refs/remotes/<peer1>/rad/self` should exist"
                );

                // check we have a top-level namespace for `owner`
                let urn = owner.urn();
                assert_eq!(
                    Some(owner),
                    identities::user::get(&storage, &urn).unwrap(),
                    "alice should be a first class citizen"
                )
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

        let TestProject { project, owner } = peer1
            .with_storage(move |storage| create_test_project(&storage))
            .await
            .unwrap()
            .unwrap();
        peer2
            .with_storage({
                let urn = project.urn();
                let peer_id = peer1.peer_id();
                move |storage| replication::replicate(&storage, None, urn, peer_id, None)
            })
            .await
            .unwrap()
            .expect("should be able to replicate");

        let peer2_events = peer2.subscribe().await;

        // Check out a working copy on peer1, add a commit, and push it
        let commit_id = block_in_place(|| {
            librad::git::local::transport::register();
            let transport_results = LocalTransportFactory::configure(local::transport::Settings {
                paths: peer1.paths().clone(),
                signer: SomeSigner { signer: peer1_key }.into(),
            });

            let tmp = tempdir().unwrap();
            let repo = git2::Repository::init(tmp.path()).unwrap();

            let mut updated_refs = Vec::new();
            let mut remote_callbacks = git2::RemoteCallbacks::new();
            remote_callbacks.push_update_reference(|refname, maybe_error| match maybe_error {
                None => {
                    let rev = repo.find_reference(refname)?.target().unwrap();
                    let refname = ext::RefLike::try_from(refname).unwrap();
                    updated_refs.push((refname, rev));

                    Ok(())
                },

                Some(err) => Err(git2::Error::from_str(&format!(
                    "Remote rejected {}: {}",
                    refname, err
                ))),
            });

            let url = LocalUrl::from_urn(project.urn(), peer1.peer_id());
            let heads = NamespacedRef::heads(Namespace::from(project.urn()), Some(peer1.peer_id()));
            let remotes = FlatRef::heads(
                PhantomData,
                ext::RefLike::try_from(format!("{}@{}", owner.subject().name, peer1.peer_id()))
                    .unwrap(),
            );

            let remote = Remote::rad_remote(url, Some(remotes.refspec(heads, Force::True).boxed()));

            let oid =
                initial_commit(&repo, remote, "refs/heads/master", Some(remote_callbacks)).unwrap();
            while let Some(results) = transport_results.wait(Duration::from_secs(1)) {
                for res in results {
                    assert_matches!(res, Ok(_), "push error");
                }
            }

            for (path, rev) in updated_refs {
                futures::executor::block_on(peer1.protocol().announce(Gossip {
                    origin: None,
                    urn: project.urn().with_path(path),
                    rev: Some(Rev::Git(rev)),
                }))
            }

            ext::Oid::from(oid)
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
                    &project.urn().with_path(
                        reflike!("refs/remotes")
                            .join(peer1.peer_id())
                            .join(reflike!("heads/master")),
                    ),
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

// FIXME(kim): does this belong here?
#[tokio::test]
async fn list_identities_returns_only_local_projects() {
    logging::init();

    const NUM_PEERS: usize = 3;

    let peers = testnet::setup(NUM_PEERS).await.unwrap();
    testnet::run_on_testnet(peers, NUM_PEERS, async move |mut apis| {
        let (peer1, _) = apis.pop().unwrap();
        let (peer2, _) = apis.pop().unwrap();
        let (peer3, _) = apis.pop().unwrap();

        let TestProject { project, .. } = peer1
            .with_storage(move |storage| create_test_project(&storage))
            .await
            .unwrap()
            .unwrap();

        peer2
            .with_storage({
                let urn = project.urn();
                let remote_peer = peer1.peer_id();
                move |storage| replication::replicate(&storage, None, urn, remote_peer, None)
            })
            .await
            .unwrap()
            .unwrap();

        let all_identities = peer3
            .with_storage({
                let urn = project.urn();
                let remote_peer = peer2.peer_id();
                move |storage| -> Result<Vec<SomeIdentity>, anyhow::Error> {
                    replication::replicate(&storage, None, urn, remote_peer, None)?;
                    Ok(identities::any::list(&storage)?.collect::<Result<Vec<_>, _>>()?)
                }
            })
            .await
            .unwrap()
            .unwrap();

        assert_eq!(2, all_identities.len());
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
        let (peer1, _) = apis.pop().unwrap();

        let TestProject { project, .. } = peer1
            .with_storage(move |storage| create_test_project(&storage))
            .await
            .unwrap()
            .unwrap();

        let (peer2, _) = apis.pop().unwrap();
        let res = peer2
            .providers(project.urn(), Duration::from_secs(5))
            .await
            .next()
            .await;

        let remote_peer = match res {
            Some(peer_info) => peer_info.peer_id,
            None => panic!("Expected to have obtained peer1 but got None instead"),
        };

        let peer2_has_urn = async || {
            peer2
                .with_storage({
                    let urn = project.urn();
                    move |storage| storage.has_urn(&urn)
                })
                .await
                .unwrap()
                .unwrap()
        };

        assert_eq!(
            false,
            peer2_has_urn().await,
            "expected peer2 to not have URN {} yet",
            project.urn()
        );

        peer2
            .with_storage({
                let urn = project.urn();
                move |storage| replication::replicate(&storage, None, urn, remote_peer, None)
            })
            .await
            .unwrap()
            .unwrap();

        assert_eq!(
            true,
            peer2_has_urn().await,
            "expected peer2 to have URN {}",
            project.urn()
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
        let (peer2, _) = apis.pop().unwrap();
        let (peer3, _) = apis.pop().unwrap();

        let TestProject { project, owner } = peer1
            .with_storage(move |storage| create_test_project(&storage))
            .await
            .unwrap()
            .unwrap();
        let default_branch = project
            .doc
            .payload
            .subject
            .default_branch
            .as_ref()
            .map(|cstring| cstring.to_string())
            .unwrap_or_else(|| "mistress".to_owned());

        let tmp = tempfile::tempdir().unwrap();
        block_in_place(|| {
            librad::git::local::transport::register();
            let transport_results = LocalTransportFactory::configure(local::transport::Settings {
                paths: peer1.paths().clone(),
                signer: SomeSigner { signer: peer1_key }.into(),
            });

            // Perform commit and push to working copy on peer1
            let repo = git2::Repository::init(tmp.path().join("peer1")).unwrap();
            let url = LocalUrl::from_urn(project.urn(), peer1.peer_id());

            let heads = NamespacedRef::heads(Namespace::from(project.urn()), Some(peer1.peer_id()));
            let remotes = FlatRef::heads(
                PhantomData,
                ext::RefLike::try_from(format!("{}@{}", owner.subject().name, peer1.peer_id()))
                    .unwrap(),
            );

            let remote = Remote::rad_remote(url, Some(remotes.refspec(heads, Force::True).boxed()));

            initial_commit(
                &repo,
                remote,
                &format!("refs/heads/{}", default_branch),
                None,
            )
            .unwrap();

            while let Some(results) = transport_results.wait(Duration::from_secs(1)) {
                for res in results {
                    assert_matches!(res, Ok(_), "push error");
                }
            }
        });

        let head = NamespacedRef::head(
            Namespace::from(project.urn()),
            peer1.peer_id(),
            ext::RefLike::try_from(default_branch.as_str()).unwrap(),
        );
        let peer2_has_ref = peer2
            .with_storage({
                let head = head.clone();
                let urn = project.urn();
                let remote_peer = peer1.peer_id();
                let addrs = Some(peer1.listen_addr());
                move |storage| -> Result<bool, anyhow::Error> {
                    replication::replicate(&storage, None, urn, remote_peer, addrs)?;
                    Ok(storage.has_ref(&head)?)
                }
            })
            .await
            .unwrap()
            .unwrap();
        let peer3_has_ref = peer3
            .with_storage({
                let head = head.clone();
                let urn = project.urn();
                let remote_peer = peer2.peer_id();
                let addrs = Some(peer2.listen_addr());
                move |storage| -> Result<bool, anyhow::Error> {
                    replication::replicate(&storage, None, urn, remote_peer, addrs)?;
                    Ok(storage.has_ref(&head)?)
                }
            })
            .await
            .unwrap()
            .unwrap();

        assert!(peer2_has_ref, format!("peer 2 missing ref `{}`", head));
        assert!(peer3_has_ref, format!("peer 3 missing ref `{}`", head));
    })
    .await;
}
