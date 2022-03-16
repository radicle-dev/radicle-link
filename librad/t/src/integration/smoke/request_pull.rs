// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::ops::Index as _;

use it_helpers::{fixed::TestProject, testnet};
use librad::{git::storage::ReadOnlyStorage as _, net::protocol::request_pull::Response};
use test_helpers::logging;

fn config() -> testnet::Config {
    testnet::Config {
        num_peers: nonzero!(2usize),
        min_connected: 2,
        bootstrap: testnet::Bootstrap::from_env(),
    }
}

#[test]
fn responds() {
    logging::init();

    let net = testnet::run(config()).unwrap();
    net.enter(async {
        let responder = net.peers().index(0);
        let requester = net.peers().index(1);
        let TestProject { project, .. } = {
            let proj = requester
                .using_storage(TestProject::create)
                .await
                .unwrap()
                .unwrap();

            proj
        };

        let mut rp = requester
            .request_pull(
                (responder.peer_id(), responder.listen_addrs().to_vec()),
                project.urn(),
            )
            .await
            .unwrap();

        while let Some(Ok(resp)) = rp.next().await {
            match resp {
                Response::Error(e) => panic!("request-pull failed: {}", e.message),
                Response::Progress(p) => tracing::debug!(progress = %p.message, "making progress"),
                Response::Success(_) => break,
            }
        }

        let pulled = responder
            .using_read_only({
                let urn = project.urn();
                move |storage| storage.has_urn(&urn)
            })
            .await
            .unwrap()
            .unwrap();

        assert!(pulled, "responder does not have project");
    })
}
