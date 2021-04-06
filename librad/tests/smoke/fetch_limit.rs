// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::git::tracking;
use librad_test::{
    logging,
    rad::{identities::TestProject, testnet},
};

#[tokio::test]
async fn replication_does_not_exceed_limit() {
    logging::init();

    const NUM_PEERS: usize = 6;

    let peers = testnet::setup(NUM_PEERS).await.unwrap();
    testnet::run_on_testnet(peers, NUM_PEERS, |mut peers| async move {
        let peer1 = peers.pop().unwrap();
        let peer2 = peers.pop().unwrap();
        let peer3 = peers.pop().unwrap();
        let peer4 = peers.pop().unwrap();
        let peer5 = peers.pop().unwrap();
        let peer6 = peers.pop().unwrap();

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

        for peer in vec![peer2, peer3, peer4, peer5].iter() {
            proj.pull(&peer1, peer).await.ok().unwrap();
            proj.pull(peer, &peer1).await.ok().unwrap();
        }
        proj.pull(&peer1, &peer6).await.ok().unwrap();
    })
    .await;
}
