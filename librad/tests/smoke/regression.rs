// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::{
    self,
    git::identities::{self, SomeIdentity},
};
use librad_test::{
    logging,
    rad::{identities::TestProject, testnet},
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

        let proj = peer1
            .using_storage(move |storage| TestProject::create(&storage))
            .await
            .unwrap()
            .unwrap();

        proj.pull(&peer1, &peer2).await.ok().unwrap();
        proj.pull(&peer2, &peer3).await.ok().unwrap();

        let all_identities = peer3
            .using_storage(move |storage| -> Result<Vec<SomeIdentity>, anyhow::Error> {
                Ok(identities::any::list(&storage)?.collect::<Result<Vec<_>, _>>()?)
            })
            .await
            .unwrap()
            .unwrap();

        assert_eq!(2, all_identities.len());
    })
    .await;
}
