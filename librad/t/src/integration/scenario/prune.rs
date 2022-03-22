// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later
use std::ops::Index as _;

use it_helpers::{fixed::TestProject, testnet};
use librad::{
    git::{
        refs::Refs,
        storage::ReadOnlyStorage,
        types::{Namespace, Reference},
        util::quick_commit,
    },
    git_ext::tree,
    reflike,
};
use test_helpers::logging;

fn config() -> testnet::Config {
    testnet::Config {
        num_peers: nonzero!(2usize),
        min_connected: 2,
        bootstrap: testnet::Bootstrap::from_env(),
    }
}

#[test]
fn receiver_prunes_deleted_refs() {
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
        let urn = proj.project.urn();

        // create two refs for peer1
        let (peer1_master, peer1_other) = peer1
            .using_storage({
                let urn = urn.clone();
                |storage| {
                    let master = quick_commit(
                        storage,
                        &urn.clone().with_path(reflike!("refs/heads/master")),
                        vec![("README", tree::blob(b"hello I am dog"))]
                            .into_iter()
                            .collect(),
                        "initial",
                    )
                    .unwrap();
                    let other = quick_commit(
                        storage,
                        &urn.with_path(reflike!("refs/heads/other")),
                        vec![("README", tree::blob(b"hello I am cat"))]
                            .into_iter()
                            .collect(),
                        "other",
                    )
                    .unwrap();

                    (master, other)
                }
            })
            .await
            .unwrap();

        // pull project from peer1 to peer2
        proj.pull(peer1, peer2).await.unwrap();

        // assert both refs were pulled
        let (peer2_master, peer2_other) = peer2
            .using_storage({
                let ns = Namespace::from(urn.clone());
                let peer1_id = peer1.peer_id();
                move |storage| {
                    let master = storage
                        .read_only()
                        .reference_oid(&Reference::head(ns.clone(), peer1_id, reflike!("master")))
                        .unwrap();
                    let other = storage
                        .read_only()
                        .reference_oid(&Reference::head(ns, peer1_id, reflike!("other")))
                        .unwrap();

                    (master, other)
                }
            })
            .await
            .unwrap();
        assert_eq!(peer1_master, peer2_master.into());
        assert_eq!(peer1_other, peer2_other.into());

        // remove 'other' ref
        peer1
            .using_storage({
                let urn = urn.clone();
                let ns = Namespace::from(urn.clone());
                move |storage| {
                    storage
                        .reference(&Reference::head(ns, None, reflike!("other")))
                        .unwrap()
                        .expect("reference 'other' exists")
                        .delete()
                        .unwrap();
                    Refs::update(storage, &urn).unwrap()
                }
            })
            .await
            .unwrap();

        // pull again
        proj.pull(peer1, peer2).await.unwrap();

        // assert 'master' is still there, but 'other' is gone
        let (peer2_master, peer2_other) = peer2
            .using_storage({
                let ns = Namespace::from(urn.clone());
                let peer1_id = peer1.peer_id();
                move |storage| {
                    let master = storage
                        .read_only()
                        .reference_oid(&Reference::head(ns.clone(), peer1_id, reflike!("master")))
                        .unwrap();
                    let other = storage
                        .read_only()
                        .reference(&Reference::head(ns, peer1_id, reflike!("other")))
                        .unwrap()
                        .map(|_| ());

                    (master, other)
                }
            })
            .await
            .unwrap();
        assert_eq!(peer1_master, peer2_master.into());
        assert!(peer2_other.is_none());
    })
}
