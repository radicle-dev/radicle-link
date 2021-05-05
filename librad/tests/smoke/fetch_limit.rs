// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::ops::Index as _;

use librad::git::tracking;
use librad_test::{
    logging,
    rad::{identities::TestProject, testnet},
};

/// Stress test the limits that are set for fetching when using `replicate`.
/// The `fetch::Limit` contains a base limit and should be scaled by the number
/// of remotes that the fetcher is fetching from.
#[test]
fn replication_does_not_exceed_limit() {
    logging::init();

    let net = testnet::run(testnet::Config {
        num_peers: nonzero!(6usize),
        min_connected: 3,
        bootstrap: testnet::Bootstrap::from_env(),
    })
    .unwrap();
    net.enter(async {
        let peer1 = net.peers().index(0);
        let peer2 = net.peers().index(1);
        let peer3 = net.peers().index(2);
        let peer4 = net.peers().index(3);
        let peer5 = net.peers().index(4);
        let peer6 = net.peers().index(5);
        let proj = peer1
            .using_storage({
                let remotes = vec![
                    peer2.peer_id(),
                    peer3.peer_id(),
                    peer4.peer_id(),
                    peer5.peer_id(),
                ];
                move |storage| {
                    let proj = TestProject::create(&storage)?;
                    for remote in remotes.into_iter() {
                        tracking::track(&storage, &proj.project.urn(), remote)?;
                    }
                    Ok::<_, anyhow::Error>(proj)
                }
            })
            .await
            .unwrap()
            .unwrap();

        for &peer in &[peer2, peer3, peer4, peer5] {
            proj.pull(peer1, peer).await.unwrap();
            proj.pull(peer, peer1).await.unwrap();
        }
        proj.pull(peer1, peer6).await.ok().unwrap();
    })
}
