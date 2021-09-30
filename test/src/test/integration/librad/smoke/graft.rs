// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{ops::Index as _, time::Duration};

use crate::{
    logging,
    rad::{identities::TestProject, testnet},
};
use librad::{
    git::{storage::ReadOnlyStorage as _, tracking, util},
    git_ext::tree,
    reflike,
};

fn config() -> testnet::Config {
    testnet::Config {
        num_peers: nonzero!(2usize),
        min_connected: 2,
        bootstrap: testnet::Bootstrap::from_env(),
    }
}

#[test]
#[ignore]
fn mutual_fetch() {
    logging::init();

    let net = testnet::run(config()).unwrap();
    net.enter(async {
        let alice = net.peers().index(0);
        let bob = net.peers().index(1);
        let project = alice
            .using_storage(move |s| TestProject::create(s))
            .await
            .unwrap()
            .unwrap();
        project.pull(alice, bob).await.unwrap();

        // Set up tracking
        alice
            .using_storage({
                let urn = project.project.urn();
                let bob = bob.peer_id();
                move |s| tracking::track(s, &urn, bob)
            })
            .await
            .unwrap()
            .unwrap();
        assert!(bob
            .using_storage({
                let urn = project.project.urn();
                let alice = alice.peer_id();
                move |s| tracking::is_tracked(&s, &urn, alice)
            })
            .await
            .unwrap()
            .unwrap());

        let commit_urn = project.project.urn().with_path(reflike!("refs/heads/hi"));
        // Create commits concurrently
        let hi_bob = alice
            .using_storage({
                let urn = commit_urn.clone();
                move |s| {
                    util::quick_commit(
                        s,
                        &urn,
                        vec![("HI", tree::blob(b"Hi Bob"))].into_iter().collect(),
                        "say hi to bob",
                    )
                }
            })
            .await
            .unwrap()
            .unwrap();
        let hi_alice = bob
            .using_storage({
                let urn = commit_urn.clone();
                move |s| {
                    util::quick_commit(
                        s,
                        &urn,
                        vec![("HI", tree::blob(b"Hi Alice"))].into_iter().collect(),
                        "say hi to alice",
                    )
                }
            })
            .await
            .unwrap()
            .unwrap();

        // Pull again
        project.pull(alice, bob).await.unwrap();
        // Wait for alice to fetch back
        tokio::time::sleep(Duration::from_secs(1)).await;

        let alice_has_bob = alice
            .using_storage({
                let urn = commit_urn.clone().map_path(|path| {
                    path.map(|path| {
                        reflike!("refs/remotes")
                            .join(bob.peer_id())
                            .join(path.strip_prefix("refs").unwrap())
                    })
                });
                move |s| s.has_commit(&urn, Box::new(hi_alice))
            })
            .await
            .unwrap()
            .unwrap();
        let bob_has_alice = bob
            .using_storage({
                let urn = commit_urn.clone().map_path(|path| {
                    path.map(|path| {
                        reflike!("refs/remotes")
                            .join(alice.peer_id())
                            .join(path.strip_prefix("refs").unwrap())
                    })
                });
                move |s| s.has_commit(&urn, Box::new(hi_bob))
            })
            .await
            .unwrap()
            .unwrap();

        assert!(alice_has_bob, "alice is missing bob's commit");
        assert!(bob_has_alice, "bob is missing alice's commit");
    })
}
