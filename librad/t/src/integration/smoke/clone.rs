// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::ops::Index as _;

use it_helpers::{
    fixed::TestProject,
    testnet::{self, RunningTestPeer},
};
use librad::{
    self,
    git::{
        identities,
        storage::ReadOnlyStorage as _,
        types::{Namespace, Reference},
    },
};
use test_helpers::logging;

fn default_config() -> testnet::Config {
    testnet::Config {
        num_peers: nonzero!(3usize),
        min_connected: 3,
        bootstrap: testnet::Bootstrap::from_env(),
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
        let voyeur_addrs = voyeur.listen_addrs().to_vec();

        contributor.clone_from(maintainer, true).await.unwrap();

        let rad_self = Reference::rad_self(Namespace::from(&urn), maintainer_id);
        // check rad/self of maintainer exists
        contributor
            .0
            .using_storage({
                let rad_self = rad_self.clone();
                move |storage| {
                    assert!(
                        storage.has_ref(&rad_self).unwrap(),
                        "`refs/remotes/<maintainer>/rad/self` should exist before"
                    );
                }
            })
            .await
            .unwrap();

        contributor
            .0
            .client()
            .unwrap()
            .replicate((voyeur_id, voyeur_addrs), urn, None)
            .await
            .expect("error replicating voyeur->contributor");

        // check again rad/self of maintainer exists
        contributor
            .0
            .using_storage(move |storage| {
                assert!(
                    storage.has_ref(&rad_self).unwrap(),
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
        Leecher(&net.peers()[1])
            .clone_from(host, false)
            .await
            .unwrap()
    })
}

#[test]
fn when_disconnected() {
    logging::init();

    let net = testnet::run(disconnected_config()).unwrap();
    net.enter(async {
        let host = Host::init(&net.peers()[0]).await;
        Leecher(&net.peers()[1])
            .clone_from(host, true)
            .await
            .unwrap()
    })
}

#[test]
fn when_disconnected_and_no_addr_hints() {
    logging::init();

    let net = testnet::run(disconnected_config()).unwrap();
    let res = net.enter(async {
        let host = Host::init(&net.peers()[0]).await;
        Leecher(&net.peers()[1]).clone_from(host, false).await
    });
    assert!(matches!(res, Err(e) if e.to_string().starts_with("unable to obtain connection to")));
}

struct Host<'a> {
    project: TestProject,
    peer: &'a RunningTestPeer,
}

impl<'a> Host<'a> {
    async fn init(peer: &'a RunningTestPeer) -> Host<'a> {
        let project = peer
            .using_storage(TestProject::create)
            .await
            .unwrap()
            .unwrap();

        Self { project, peer }
    }
}

struct Leecher<'a>(&'a RunningTestPeer);

impl Leecher<'_> {
    async fn clone_from(&self, host: Host<'_>, supply_addr_hints: bool) -> anyhow::Result<()> {
        let urn = host.project.project.urn();
        let owner = host.project.owner;
        let host_peer = host.peer.peer_id();
        let host_addrs = host.peer.listen_addrs().to_vec();

        self.0
            .client()?
            .replicate(
                (
                    host_peer,
                    supply_addr_hints
                        .then(|| host_addrs)
                        .into_iter()
                        .flatten()
                        .collect(),
                ),
                urn.clone(),
                None,
            )
            .await?;
        self.0
            .using_storage(move |storage| {
                // check rad/self of peer1 exists
                {
                    let has_ref =
                        storage.has_ref(&Reference::rad_self(Namespace::from(&urn), host_peer))?;
                    anyhow::ensure!(has_ref, "`refs/remotes/<peer1>/rad/self` should exist");
                }

                // check we have a top-level namespace for `owner`
                {
                    let urn = owner.urn();
                    let pers = identities::person::get(&storage, &urn)?;
                    anyhow::ensure!(pers == Some(owner), "alice should be a first class citizen");
                }

                Ok(())
            })
            .await??;

        Ok(())
    }
}
