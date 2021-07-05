// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use crate::{
    logging,
    rad::{identities::TestProject, testnet},
};
use lazy_static::lazy_static;
use librad::{
    git::{
        identities,
        storage::ReadOnlyStorage,
        tracking,
        types::{Namespace, Reference},
        util::quick_commit,
    },
    git_ext::tree,
    keys::SecretKey,
    reflike,
};
use std::ops::Index as _;

lazy_static! {
    static ref KEY_ONE: SecretKey = SecretKey::from_seed([
        100, 107, 14, 43, 237, 25, 113, 215, 236, 197, 160, 60, 169, 174, 81, 58, 143, 74, 42, 201,
        122, 252, 143, 21, 82, 225, 111, 252, 12, 186, 4, 154
    ]);
    static ref KEY_TWO: SecretKey = SecretKey::from_seed([
        153, 72, 253, 68, 81, 29, 234, 67, 15, 241, 138, 59, 180, 75, 76, 113, 103, 189, 174, 200,
        244, 183, 138, 215, 98, 231, 103, 194, 0, 53, 124, 119
    ]);
}

fn config() -> testnet::Config {
    testnet::Config {
        num_peers: nonzero!(2usize),
        min_connected: 2,
        bootstrap: testnet::Bootstrap::from_env(),
    }
}

#[test]
fn rere_tracked() {
    logging::init();

    let net = testnet::run(config()).unwrap();
    let peer1 = net.peers().index(0).clone();
    let peer2 = net.peers().index(1).clone();
    net.enter(async {
        tracing::info!(peer1=?peer1.peer_id(), peer2=?peer2.peer_id(), "created peers");

        let proj = peer1
            .using_storage(move |storage| TestProject::create(&storage))
            .await
            .unwrap()
            .unwrap();

        tracing::info!(project_id=?proj.project.urn().encode_id(), "created project");

        peer1
            .using_storage({
                let urn = proj.project.urn();
                let peer2_id = peer2.peer_id().clone();
                move |storage| {
                    let id = identities::local::load(storage, urn.clone())
                        .expect("local ID should have been created by TestProject::create")
                        .unwrap();
                    id.link(storage, &urn).unwrap();
                    tracking::track(storage, &urn, peer2_id).unwrap();
                }
            })
            .await
            .unwrap();

        // Create a commit in peer1's view of the project
        // Note that if we don't create a commit in peer 1 the test passes
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

        tracing::debug!("pulling");
        let peer1_storage_path = peer1
            .using_storage(|s| s.path().to_path_buf())
            .await
            .unwrap();
        let peer2_storage_path = peer2
            .using_storage(|s| s.path().to_path_buf())
            .await
            .unwrap();
        tracing::debug!(
            ?peer1_storage_path,
            ?peer2_storage_path,
            "STORAGESTORAGEGETYOURSTORAGE"
        );

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
