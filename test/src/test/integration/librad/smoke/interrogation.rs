// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{ops::Index as _, time::Duration};

use librad::{
    data::BoundedVec,
    identities::SomeUrn,
    net::protocol::{
        event::{self, upstream::predicate},
        PeerAdvertisement,
    },
};

use crate::{
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

#[test]
fn responds() {
    logging::init();

    let net = testnet::run(config()).unwrap();
    net.enter(async {
        let responder = net.peers().index(0);
        let requester = net.peers().index(1);
        let TestProject { project, owner } = {
            let events = responder.subscribe();
            let proj = responder
                .using_storage(move |s| TestProject::create(&s))
                .await
                .unwrap()
                .unwrap();

            // Make sure responder had a chance to refresh its caches
            let stats = responder.stats().await;
            if stats.caches.urns.elements < 2 {
                tracing::debug!(
                    "waiting for cache rebuild (expected 2 elements, got {})",
                    stats.caches.urns.elements
                );
                futures::pin_mut!(events);
                event::upstream::expect(
                    events,
                    predicate::urn_cache_len(|len| len >= 2),
                    Duration::from_secs(1),
                )
                .await
                .unwrap();
            }

            proj
        };

        let interrogation =
            requester.interrogate((responder.peer_id(), responder.listen_addrs().to_vec()));
        assert_eq!(
            PeerAdvertisement {
                listen_addrs: BoundedVec::try_from_length(
                    responder.listen_addrs().iter().copied().collect()
                )
                .unwrap(),
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
            assert!(urns.contains(urn), "{} not in set", urn)
        }
    })
}
