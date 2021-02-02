// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom, time::Duration};

use futures::StreamExt as _;
use librad::{
    git::{
        local::url::LocalUrl,
        replication,
        types::{remote, Flat, Force, GenericRef, Namespace, Reference, Refspec, Remote},
    },
    git_ext as ext,
    net::protocol::{
        event::{self, upstream::predicate::gossip_from},
        gossip::{self, Rev},
    },
    reflike,
    refspec_pattern,
};
use librad_test::{
    git::create_commit,
    logging,
    rad::{
        identities::{create_test_project, TestProject},
        testnet,
    },
};
use tempfile::tempdir;

const NUM_PEERS: usize = 2;

#[tokio::test(core_threads = 2)]
async fn fetches_on_gossip_notify() {
    logging::init();

    let peers = testnet::setup(NUM_PEERS).await.unwrap();
    testnet::run_on_testnet(peers, NUM_PEERS, |mut peers| async move {
        let peer1 = peers.pop().unwrap();
        let peer2 = peers.pop().unwrap();

        let TestProject { project, owner } = peer1
            .using_storage(move |storage| create_test_project(&storage))
            .await
            .unwrap()
            .unwrap();
        peer2
            .using_storage({
                let urn = project.urn();
                let peer_id = peer1.peer_id();
                let cfg = peer2.protocol_config().replication;
                move |storage| replication::replicate(&storage, cfg, None, urn, peer_id, None)
            })
            .await
            .unwrap()
            .expect("should be able to replicate");

        let peer2_events = peer2.subscribe();

        let mastor = reflike!("refs/heads/master");
        // Check out a working copy on peer1, add a commit, and push it
        let commit_id = {
            let tmp = tempdir().unwrap();
            let repo = git2::Repository::init(tmp.path()).unwrap();
            let url = LocalUrl::from(project.urn());

            let mut remote = Remote::rad_remote(
                url,
                Refspec {
                    src: Reference::heads(Namespace::from(project.urn()), peer1.peer_id()),
                    dst: GenericRef::heads(
                        Flat,
                        ext::RefLike::try_from(format!(
                            "{}@{}",
                            owner.subject().name,
                            peer1.peer_id(),
                        ))
                        .unwrap(),
                    ),
                    force: Force::True,
                }
                .into_fetchspec(),
            );

            let oid = create_commit(&repo, mastor.clone()).unwrap();
            remote
                .push(
                    peer1.clone(),
                    &repo,
                    remote::LocalPushspec::Matching {
                        pattern: refspec_pattern!("refs/heads/*"),
                        force: Force::True,
                    },
                )
                .unwrap()
                .for_each(drop);
            peer1
                .announce(gossip::Payload {
                    origin: None,
                    urn: project.urn().with_path(mastor.clone()),
                    rev: Some(Rev::Git(oid)),
                })
                .unwrap();

            oid
        };

        // Wait for peer2 to receive the gossip announcement
        event::upstream::expect(
            peer2_events,
            gossip_from(peer1.peer_id()),
            Duration::from_secs(5),
        )
        .await
        .unwrap();

        // Check that peer2 has fetched the update
        let peer2_has_commit = peer2
            .using_storage(move |storage| {
                storage.has_commit(
                    &project.urn().with_path(
                        reflike!("refs/remotes")
                            .join(peer1.peer_id())
                            .join(mastor.strip_prefix("refs").unwrap()),
                    ),
                    Box::new(commit_id),
                )
            })
            .await
            .unwrap()
            .unwrap();
        assert!(peer2_has_commit);
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

    let peers = testnet::setup(NUM_PEERS).await.unwrap();
    testnet::run_on_testnet(peers, NUM_PEERS, |mut peers| async move {
        let peer1 = peers.pop().unwrap();

        let TestProject { project, .. } = peer1
            .using_storage(move |storage| create_test_project(&storage))
            .await
            .unwrap()
            .unwrap();

        let peer2 = peers.pop().unwrap();
        let res = peer2
            .providers(project.urn(), Duration::from_secs(5))
            .next()
            .await;

        let remote_peer = match res {
            Some(peer_info) => peer_info.peer_id,
            None => panic!("Expected to have obtained peer1 but got None instead"),
        };

        let peer2_has_urn = || async {
            peer2
                .using_storage({
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
            .using_storage({
                let urn = project.urn();
                let cfg = peer2.protocol_config().replication;
                move |storage| replication::replicate(&storage, cfg, None, urn, remote_peer, None)
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
