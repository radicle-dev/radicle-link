// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::ops::Index as _;

use librad::{
    git::{
        identities,
        storage::ReadOnlyStorage,
        tracking,
        types::{Namespace, Reference},
        util::quick_commit,
    },
    git_ext::tree,
    reflike,
};
use test_helpers::logging;

use crate::rad::{identities::TestProject, testnet};

fn config() -> testnet::Config {
    testnet::Config {
        num_peers: nonzero!(2usize),
        min_connected: 2,
        bootstrap: testnet::Bootstrap::from_env(),
    }
}

/// Peers should be able to see references created by peers in their tracking
/// graph. To test this we do the following:
///
/// - Create a testnet with two peers
/// - Make peer1 track peer2
/// - Create a project in peer1s storage
/// - Create a commit in the project as peer1, point refs/heads/master at it
/// - Pull the project from peer1 to peer2
/// - Create a commit in peer2, point refs/heads/master at it
/// - Pull the project from peer2 into peer1
/// - Assert that the reference refs/namespaces/<project>/refs/remotes/<Peer
///   2>/heads/master exists in peer1s storage.
///
/// This test was initially created to reproduce this issue
/// https://github.com/radicle-dev/radicle-link/issues/726 which was due to
/// the patch we have applied to libgit incorrectly handling unknown remote
/// references. This is now fixed but it seems prudent to retain the test as it
/// exercises a critical code path.
#[test]
fn can_see_tracked_references() {
    logging::init();

    let net = testnet::run(config()).unwrap();
    net.enter(async {
        let peer1 = net.peers().index(0);
        let peer2 = net.peers().index(1);

        let proj = peer1
            .using_storage(TestProject::create)
            .await
            .unwrap()
            .unwrap();

        peer1
            .using_storage({
                let urn = proj.project.urn();
                let peer2_id = peer2.peer_id();
                move |storage| {
                    let id = identities::local::load(storage, urn.clone())
                        .expect("local ID should have been created by TestProject::create")
                        .unwrap();
                    id.link(storage, &urn).unwrap();
                    assert!(tracking::track(
                        storage,
                        &urn,
                        Some(peer2_id),
                        tracking::Config::default(),
                        tracking::policy::Track::Any,
                    )
                    .unwrap()
                    .is_ok());
                }
            })
            .await
            .unwrap();

        // Create a commit in peer1's view of the project
        let _peer1_commit_id = peer1
            .using_storage({
                let urn = proj.project.urn();
                |storage| {
                    quick_commit(
                        storage,
                        &urn.with_path(reflike!("refs/heads/master")),
                        vec![("HI", tree::blob(b"Hi Bob"))].into_iter().collect(),
                        "say hi to bob",
                    )
                }
            })
            .await
            .unwrap()
            .unwrap();

        // pull project from peer1 to peer2
        proj.pull(peer1, peer2).await.unwrap();

        // create a commit in proj from peer2
        let peer2_commit_id = peer2
            .using_storage({
                let urn = proj.project.urn();
                |storage| {
                    quick_commit(
                        storage,
                        &urn.with_path(reflike!("refs/heads/master")),
                        vec![("HI", tree::blob(b"Hi Bob"))].into_iter().collect(),
                        "say hi to bob",
                    )
                }
            })
            .await
            .unwrap()
            .unwrap();

        // pull project from peer2 to peer1
        proj.pull(peer2, peer1).await.unwrap();

        // assert that peer2's remote in peer1 points to the above commit
        peer1
            .using_storage({
                let urn = proj.project.urn();
                let peer2_id = peer2.peer_id();
                move |storage| {
                    let rad_ref =
                        Reference::head(Namespace::from(urn.clone()), peer2_id, reflike!("master"));
                    let the_ref = storage.read_only().reference(&rad_ref).unwrap().unwrap();
                    assert_eq!(the_ref.target().unwrap(), peer2_commit_id);
                }
            })
            .await
            .unwrap();
    })
}
