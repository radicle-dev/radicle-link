// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::{
    self,
    git::{
        identities::{self, SomeIdentity},
        replication,
    },
};
use librad_test::{
    logging,
    rad::{
        identities::{create_test_project, TestProject},
        testnet,
    },
};

/// https://github.com/radicle-dev/radicle-link/issues/250
/// Fixed in: 577e9943fa704895b47fe4e1c862bf0bd51d58a9
#[tokio::test]
async fn list_identities_returns_only_local_projects() {
    logging::init();

    const NUM_PEERS: usize = 3;

    let peers = testnet::setup(NUM_PEERS).await.unwrap();
    testnet::run_on_testnet(peers, NUM_PEERS, |mut peers| async move {
        let peer1 = peers.pop().unwrap();
        let peer2 = peers.pop().unwrap();
        let peer3 = peers.pop().unwrap();

        let TestProject { project, .. } = peer1
            .using_storage(move |storage| create_test_project(&storage))
            .await
            .unwrap()
            .unwrap();

        peer2
            .using_storage({
                let urn = project.urn();
                let remote_peer = peer1.peer_id();
                let cfg = peer2.protocol_config().replication;
                move |storage| replication::replicate(&storage, cfg, None, urn, remote_peer, None)
            })
            .await
            .unwrap()
            .unwrap();

        let all_identities = peer3
            .using_storage({
                let urn = project.urn();
                let remote_peer = peer2.peer_id();
                let cfg = peer3.protocol_config().replication;
                move |storage| -> Result<Vec<SomeIdentity>, anyhow::Error> {
                    replication::replicate(&storage, cfg, None, urn, remote_peer, None)?;
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
