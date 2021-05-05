// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::ops::Index as _;

use crate::{
    logging,
    rad::{identities::TestProject, testnet},
};
use librad::{
    self,
    git::identities::{self, SomeIdentity},
};

/// https://github.com/radicle-dev/radicle-link/issues/250
/// Fixed in: 577e9943fa704895b47fe4e1c862bf0bd51d58a9
#[test]
fn list_identities_returns_only_local_projects() {
    logging::init();

    let net = testnet::run(testnet::Config {
        num_peers: nonzero!(3usize),
        min_connected: 3,
        bootstrap: testnet::Bootstrap::from_env(),
        overrides: Default::default(),
    })
    .unwrap();
    net.enter(async {
        let peer1 = net.peers().index(0);
        let peer2 = net.peers().index(1);
        let peer3 = net.peers().index(2);
        let proj = peer1
            .using_storage(move |storage| TestProject::create(&storage))
            .await
            .unwrap()
            .unwrap();

        proj.pull(peer1, peer2).await.ok().unwrap();
        proj.pull(peer2, peer3).await.ok().unwrap();

        let all_identities = peer3
            .using_storage(move |storage| -> Result<Vec<SomeIdentity>, anyhow::Error> {
                Ok(identities::any::list(&storage)?.collect::<Result<Vec<_>, _>>()?)
            })
            .await
            .unwrap()
            .unwrap();

        assert_eq!(2, all_identities.len());
    })
}
