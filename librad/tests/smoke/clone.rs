// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::{
    self,
    git::{
        identities,
        replication,
        types::{Namespace, Reference},
    },
};
use librad_test::{
    logging,
    rad::{
        identities::TestProject,
        testnet::{self, RunningTestPeer},
    },
};

const NUM_PEERS: usize = 2;

#[tokio::test]
async fn when_connected() {
    logging::init();

    let peers = testnet::setup(NUM_PEERS).await.unwrap();
    testnet::run_on_testnet(peers, NUM_PEERS, |mut peers| async move {
        let host = Host::init(peers.pop().unwrap()).await;
        Leecher(peers.pop().unwrap()).clone_from(host, false).await
    })
    .await;
}

#[tokio::test]
async fn when_disconnected() {
    logging::init();

    let peers = testnet::setup_disconnected(NUM_PEERS).await.unwrap();
    testnet::run_on_testnet(peers, 0, |mut peers| async move {
        let host = Host::init(peers.pop().unwrap()).await;
        Leecher(peers.pop().unwrap()).clone_from(host, true).await
    })
    .await;
}

#[tokio::test]
#[should_panic(expected = "No connection")]
async fn when_disconnected_and_no_addr_hints() {
    logging::init();

    let peers = testnet::setup_disconnected(NUM_PEERS).await.unwrap();
    testnet::run_on_testnet(peers, 0, |mut peers| async move {
        let host = Host::init(peers.pop().unwrap()).await;
        Leecher(peers.pop().unwrap()).clone_from(host, false).await
    })
    .await;
}

struct Host {
    project: TestProject,
    peer: RunningTestPeer,
}

impl Host {
    async fn init(peer: RunningTestPeer) -> Self {
        let project = peer
            .using_storage(move |storage| TestProject::create(&storage))
            .await
            .unwrap()
            .unwrap();

        Self { project, peer }
    }
}

struct Leecher(RunningTestPeer);

impl Leecher {
    async fn clone_from(&self, host: Host, supply_addr_hints: bool) {
        let cfg = self.0.protocol_config().replication;
        self.0
            .using_storage(move |storage| {
                let urn = host.project.project.urn();
                replication::replicate(
                    &storage,
                    cfg,
                    None,
                    urn.clone(),
                    host.peer.peer_id(),
                    supply_addr_hints
                        .then_some(host.peer.listen_addrs().iter().copied())
                        .into_iter()
                        .flatten(),
                )
                .unwrap();

                // check rad/self of peer1 exists
                assert!(
                    storage
                        .has_ref(&Reference::rad_self(
                            Namespace::from(&urn),
                            host.peer.peer_id()
                        ))
                        .unwrap(),
                    "`refs/remotes/<peer1>/rad/self` should exist"
                );

                // check we have a top-level namespace for `owner`
                let owner = host.project.owner;
                let urn = owner.urn();
                assert_eq!(
                    Some(owner),
                    identities::person::get(&storage, &urn).unwrap(),
                    "alice should be a first class citizen"
                )
            })
            .await
            .unwrap();
    }
}
