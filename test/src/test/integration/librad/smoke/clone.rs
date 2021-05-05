// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::ops::Index as _;

use crate::{
    logging,
    rad::{
        identities::TestProject,
        testnet::{self, RunningTestPeer},
    },
};
use librad::{
    self,
    git::{
        identities,
        replication,
        storage::fetcher,
        types::{Namespace, Reference},
    },
};

fn default_config() -> testnet::Config {
    testnet::Config {
        num_peers: nonzero!(3usize),
        min_connected: 3,
        bootstrap: testnet::Bootstrap::from_env(),
        overrides: Default::default(),
    }
}

fn disconnected_config() -> testnet::Config {
    testnet::Config {
        min_connected: 0,
        bootstrap: testnet::Bootstrap::None,
        ..default_config()
    }
}

/// Fetching from a peer that does not have the identity should leave the
/// `rad/*` refs intact.
#[test]
fn not_present() {
    logging::init();

    let net = testnet::run(default_config()).unwrap();
    net.enter(async {
        let maintainer = Host::init(net.peers().index(0)).await;
        let contributor = Leecher(net.peers().index(1));
        let voyeur = net.peers().index(2);

        let urn = maintainer.project.project.urn().clone();
        let maintainer_id = maintainer.peer.peer_id();
        let voyeur_id = voyeur.peer_id();
        let voyeur_addrs = voyeur.listen_addrs().iter().copied().collect::<Vec<_>>();

        contributor.clone_from(maintainer, true).await;

        let cfg = contributor.0.protocol_config().replication;
        contributor
            .0
            .using_storage(move |storage| {
                // check rad/self of maintainer exists
                assert!(
                    storage
                        .has_ref(&Reference::rad_self(Namespace::from(&urn), maintainer_id))
                        .unwrap(),
                    "`refs/remotes/<maintainer>/rad/self` should exist before"
                );

                let fetcher = fetcher::PeerToPeer::new(urn.clone(), voyeur_id, voyeur_addrs)
                    .build(&storage)
                    .unwrap()
                    .unwrap();
                let res = replication::replicate(&storage, fetcher, cfg, None);
                assert!(res.is_ok());

                // check rad/self of maintainer exists
                assert!(
                    storage
                        .has_ref(&Reference::rad_self(Namespace::from(&urn), maintainer_id))
                        .unwrap(),
                    "`refs/remotes/<maintainer>/rad/self` should exist after"
                );
            })
            .await
            .unwrap();
    })
}

#[test]
fn when_connected() {
    logging::init();

    let net = testnet::run(default_config()).unwrap();
    net.enter(async {
        let host = Host::init(&net.peers()[0]).await;
        Leecher(&net.peers()[1]).clone_from(host, false).await
    })
}

#[test]
fn when_disconnected() {
    logging::init();

    let net = testnet::run(disconnected_config()).unwrap();
    net.enter(async {
        let host = Host::init(&net.peers()[0]).await;
        Leecher(&net.peers()[1]).clone_from(host, true).await
    })
}

#[test]
#[should_panic(expected = "git p2p transport: no connection to")]
fn when_disconnected_and_no_addr_hints() {
    logging::init();

    let net = testnet::run(disconnected_config()).unwrap();
    net.enter(async {
        let host = Host::init(&net.peers()[0]).await;
        Leecher(&net.peers()[1]).clone_from(host, false).await
    })
}

struct Host<'a> {
    project: TestProject,
    peer: &'a RunningTestPeer,
}

impl<'a> Host<'a> {
    async fn init(peer: &'a RunningTestPeer) -> Host<'a> {
        let project = peer
            .using_storage(move |storage| TestProject::create(&storage))
            .await
            .unwrap()
            .unwrap();

        Self { project, peer }
    }
}

struct Leecher<'a>(&'a RunningTestPeer);

impl Leecher<'_> {
    async fn clone_from(&self, host: Host<'_>, supply_addr_hints: bool) {
        let cfg = self.0.protocol_config().replication;
        let urn = host.project.project.urn();
        let owner = host.project.owner;
        let host_peer = host.peer.peer_id();
        let host_addrs = host.peer.listen_addrs().iter().copied().collect::<Vec<_>>();
        self.0
            .using_storage(move |storage| {
                let fetcher = fetcher::PeerToPeer::new(
                    urn.clone(),
                    host_peer,
                    supply_addr_hints
                        .then_some(host_addrs)
                        .into_iter()
                        .flatten(),
                )
                .build(&storage)
                .unwrap()
                .unwrap();
                replication::replicate(&storage, fetcher, cfg, None).unwrap();

                // check rad/self of peer1 exists
                assert!(
                    storage
                        .has_ref(&Reference::rad_self(Namespace::from(&urn), host_peer))
                        .unwrap(),
                    "`refs/remotes/<peer1>/rad/self` should exist"
                );

                // check we have a top-level namespace for `owner`
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
