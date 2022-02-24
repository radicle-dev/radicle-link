// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::time::{Duration, SystemTime};

use futures::{future, StreamExt as _};
use librad::identities::payload::{Person, PersonPayload};
use radicle_daemon::{peer::run_config, seed::Seed, state, RunConfig};
use test_helpers::logging;
use tokio::time::timeout;

use crate::common::{assert_cloned, radicle_project, requested, shia_le_pathbuf, Harness, TestExt};

#[test]
fn can_observe_announcement_from_connected_peer() -> Result<(), anyhow::Error> {
    logging::init();

    let mut harness = Harness::new();
    let alice_peer = harness.add_peer(
        "alice",
        RunConfig {
            announce: run_config::Announce {
                interval: Duration::from_millis(100),
            },
            ..RunConfig::default()
        },
        &[],
    )?;
    let mut bob_peer = harness.add_peer(
        "bob",
        RunConfig::default(),
        &[Seed {
            addrs: alice_peer.listen_addrs.clone(),
            peer_id: alice_peer.peer_id,
        }],
    )?;
    harness.enter(async move {
        let project = state::init_project(
            &alice_peer.peer,
            &alice_peer.owner,
            shia_le_pathbuf(alice_peer.path.join("radicle")),
        )
        .await?;
        let project_id = project.urn().id;
        let announced = async_stream::stream! { loop { yield bob_peer.events.recv().await } }
            .filter_map(|res| match res.unwrap() {
                radicle_daemon::PeerEvent::GossipFetched {
                    gossip, provider, ..
                } if provider.peer_id == alice_peer.peer_id && gossip.urn.id == project_id => {
                    future::ready(Some(()))
                },
                _ => future::ready(None),
            })
            .map(|_| ());
        tokio::pin!(announced);
        timeout(Duration::from_secs(5), announced.next()).await?;

        Ok(())
    })
}

#[test]
fn can_observe_person_announcement_from_connected_peer() -> Result<(), anyhow::Error> {
    logging::init();

    let mut harness = Harness::new();
    let alice_peer = harness.add_peer(
        "alice",
        RunConfig {
            announce: run_config::Announce {
                interval: Duration::from_millis(100),
            },
            ..RunConfig::default()
        },
        &[],
    )?;
    let mut bob_peer = harness.add_peer(
        "bob",
        RunConfig::default(),
        &[Seed {
            addrs: alice_peer.listen_addrs.clone(),
            peer_id: alice_peer.peer_id,
        }],
    )?;
    harness.enter(async move {
        let person = Person {
            name: "alice".into(),
        };
        let ext = TestExt("test".to_string());
        let payload = PersonPayload::new(person).with_ext(ext)?;
        state::update_owner_payload(&alice_peer.peer, payload).await?;

        let announced = async_stream::stream! { loop { yield bob_peer.events.recv().await } }
            .filter_map(|res| match res.unwrap() {
                radicle_daemon::PeerEvent::GossipFetched {
                    gossip, provider, ..
                } if provider.peer_id == alice_peer.peer_id
                    && gossip.urn.id == alice_peer.owner.root =>
                {
                    future::ready(Some(()))
                },
                _ => future::ready(None),
            })
            .map(|_| ());
        tokio::pin!(announced);
        timeout(Duration::from_secs(5), announced.next()).await?;

        Ok(())
    })
}

#[test]
fn can_ask_and_clone_project() -> Result<(), anyhow::Error> {
    logging::init();

    let mut harness = Harness::new();
    let mut alice_peer = harness.add_peer("alice", RunConfig::default(), &[])?;
    let mut bob_peer = harness.add_peer(
        "bob",
        RunConfig::default(),
        &[Seed {
            addrs: alice_peer.listen_addrs.clone(),
            peer_id: alice_peer.peer_id,
        }],
    )?;
    harness.enter(async move {
        let urn = {
            let project = radicle_project(alice_peer.path.join("radicle"));
            let urn = state::init_project(&alice_peer.peer, &alice_peer.owner, project)
                .await?
                .urn();

            urn
        };

        // Alice will track Bob in anticipation of upcoming contributions.
        state::track(&alice_peer.peer, urn.clone(), bob_peer.peer_id).await?;

        // Make sure Bob is NotReplicated.
        assert_eq!(
            state::tracked(&alice_peer.peer, urn.clone()).await?,
            vec![radicle_daemon::project::peer::Peer::Remote {
                peer_id: bob_peer.peer_id,
                status: radicle_daemon::project::peer::Status::NotReplicated,
            }]
        );

        bob_peer
            .control
            .request_project(&urn, SystemTime::now())
            .await;

        requested(&mut bob_peer.events, &urn).await?;
        assert_cloned(&mut bob_peer.events, &urn.clone(), alice_peer.peer_id).await?;
        state::get_project(&bob_peer.peer, urn.clone()).await?;

        timeout(Duration::from_secs(5), async {
            loop {
                let evt = alice_peer.events.recv().await?;
                match evt {
                    radicle_daemon::PeerEvent::GossipFetched {
                        gossip, provider, ..
                    } if provider.peer_id == bob_peer.peer_id && gossip.urn.id == urn.id => break,

                    _ => continue,
                }
            }

            Ok::<_, anyhow::Error>(())
        })
        .await??;

        let projects = state::list_projects(&bob_peer.peer).await?;
        assert_eq!(projects.len(), 1);

        let alice_tracked = state::tracked(&alice_peer.peer, urn.clone()).await?;

        match alice_tracked.first().unwrap() {
            radicle_daemon::project::peer::Peer::Remote {
                peer_id,
                status:
                    radicle_daemon::project::peer::Status::Replicated(
                        radicle_daemon::project::peer::Replicated { role, .. },
                    ),
            } => {
                assert_eq!(peer_id, &bob_peer.peer_id);
                assert_eq!(role, &radicle_daemon::project::peer::Role::Tracker);
            },
            _ => unreachable!(),
        }

        Ok(())
    })
}
