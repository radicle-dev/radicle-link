// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{ops::Index as _, time::Duration};

use futures::StreamExt as _;
use librad::{
    git::{storage::ReadOnlyStorage as _, tracking},
    net::protocol::{
        event::{self, upstream::predicate::gossip_from},
        gossip,
    },
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

#[test]
fn can_replicate_from_tracking() {
    logging::init();

    let net = testnet::run(config()).unwrap();
    net.enter(async {
        let peer1 = net.peers().index(0);
        let peer2 = net.peers().index(1);

        let proj = {
            let proj = peer1
                .using_storage(TestProject::create)
                .await
                .unwrap()
                .unwrap();
            proj.pull(peer1, peer2).await.unwrap();
            proj
        };

        peer2
            .using_storage({
                let urn = proj.project.urn();
                move |storage| {
                    tracking::track(
                        storage,
                        &urn,
                        None,
                        tracking::Config::default(),
                        tracking::policy::Track::MustNotExist,
                    )
                }
            })
            .await
            .unwrap()
            .unwrap()
            .unwrap();

        peer1
            .announce(gossip::Payload {
                origin: None,
                urn: proj.project.urn(),
                rev: None,
            })
            .unwrap();

        let peer2_events = peer2.subscribe();
        event::upstream::expect(
            peer2_events.boxed(),
            gossip_from(peer1.peer_id()),
            Duration::from_secs(15),
        )
        .await
        .unwrap();

        let has_proj = peer2
            .using_storage({
                let urn = proj.project.urn();
                move |storage| storage.has_urn(&urn)
            })
            .await
            .unwrap()
            .unwrap();
        assert!(has_proj);
    })
}
