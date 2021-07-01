// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{ops::Index as _, time::Duration};

use futures::StreamExt;

use crate::{
    logging,
    rad::{identities::TestProject, testnet},
};
use librad::{
    git::{identities, tracking},
    identities::payload,
    net::protocol::{
        event::{self, upstream::predicate::gossip_from},
        gossip,
    },
};

fn config() -> testnet::Config {
    testnet::Config {
        num_peers: nonzero!(2usize),
        min_connected: 2,
        bootstrap: testnet::Bootstrap::from_env(),
    }
}

#[test]
fn saturate_a_peer_with_projects() {
    logging::init();

    const NUM_PROJECTS: usize = 64;

    let net = testnet::run(config()).unwrap();
    net.enter(async {
        let peer1 = net.peers().index(0);
        let peer2 = net.peers().index(1);

        let payloads = (1..NUM_PROJECTS).into_iter().map(|n| payload::Project {
            name: format!("radicle-{}", n).into(),
            description: None,
            default_branch: Some(format!("rad-{}", n).into()),
        });
        let projs = peer1
            .using_storage({
                move |storage| {
                    let mut projs = Vec::with_capacity(NUM_PROJECTS + 1);
                    let proj = TestProject::create(storage)?;
                    let owner = proj.owner.clone();
                    projs.push(proj);
                    for payload in payloads {
                        projs.push(TestProject::from_project_payload(
                            storage,
                            owner.clone(),
                            payload,
                        )?);
                    }
                    Ok::<_, anyhow::Error>(projs)
                }
            })
            .await
            .unwrap()
            .unwrap();
        peer2
            .using_storage({
                let remote = peer1.peer_id();
                let urns = projs
                    .iter()
                    .map(|proj| proj.project.urn())
                    .collect::<Vec<_>>();
                move |storage| -> Result<(), anyhow::Error> {
                    for urn in urns {
                        tracking::track(storage, &urn, remote)?;
                    }
                    Ok(())
                }
            })
            .await
            .unwrap()
            .unwrap();

        for proj in projs.iter() {
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
                Duration::from_secs(5),
            )
            .await
            .unwrap();
        }

        let n_projects = peer2
            .using_storage(move |storage| -> Result<usize, anyhow::Error> {
                Ok(identities::any::list(&storage)?
                    .filter_map(|some| some.unwrap().project())
                    .count())
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(n_projects, NUM_PROJECTS);
    })
}
