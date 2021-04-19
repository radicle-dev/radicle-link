// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::ops::Index as _;

use librad::{identities::SomeUrn, net::protocol::PeerAdvertisement};
use librad_test::{
    logging,
    rad::{identities::TestProject, testnet},
};

fn config() -> testnet::Config {
    testnet::Config {
        num_peers: nonzero!(2usize),
        min_connected: 2,
        bootstrap: testnet::Bootstrap::from_env(),
    }
}

#[tokio::test]
async fn responds() {
    logging::init();

    let net = testnet::run(config()).await.unwrap();
    {
        let responder = net.peers().index(0);
        let requester = net.peers().index(1);
        let TestProject { project, owner } = responder
            .using_storage(move |s| TestProject::create(&s))
            .await
            .unwrap()
            .unwrap();

        let interrogation =
            requester.interrogate((responder.peer_id(), responder.listen_addrs().to_vec()));
        assert_eq!(
            PeerAdvertisement {
                listen_addrs: responder.listen_addrs().iter().copied().collect(),
                capabilities: Default::default(),
            },
            interrogation.peer_advertisement().await.unwrap()
        );
        assert_eq!(
            requester.listen_addrs()[0],
            interrogation.echo_addr().await.unwrap()
        );
        let urns = interrogation.urns().await.unwrap();
        for urn in &[SomeUrn::Git(project.urn()), SomeUrn::Git(owner.urn())] {
            assert!(urns.contains(urn))
        }
    }
}
