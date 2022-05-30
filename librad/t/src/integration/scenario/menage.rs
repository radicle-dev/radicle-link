// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryInto, ops::Index as _};

use blocking::unblock;
use git_ref_format::RefString;
use it_helpers::{
    fixed::{self, TestProject},
    layout,
    testnet,
};
use librad::{
    self,
    git::{refs::Refs, tracking},
    git_ext as ext,
    reflike,
};
use test_helpers::logging;

fn config() -> testnet::Config {
    testnet::Config {
        num_peers: nonzero!(3usize),
        min_connected: 3,
        bootstrap: testnet::Bootstrap::from_env(),
    }
}

#[test]
fn a_trois() {
    logging::init();

    let net = testnet::run(config()).unwrap();
    net.enter(async {
        let peer1 = net.peers().index(0);
        let peer2 = net.peers().index(1);
        let peer3 = net.peers().index(2);

        let proj = peer1
            .using_storage(TestProject::create)
            .await
            .unwrap()
            .unwrap();
        let default_branch: RefString = proj
            .project
            .doc
            .payload
            .subject
            .default_branch
            .as_ref()
            .map(|cstring| cstring.to_string())
            .unwrap_or_else(|| "mistress".to_owned())
            .try_into()
            .unwrap();
        let repo = fixed::repository(peer1.peer_id());
        let commit_id = unblock(fixed::commit(
            (*peer1).clone(),
            repo,
            &proj.project,
            &proj.owner,
            default_branch.clone(),
        ))
        .await;

        let expected_urn = proj.project.urn().with_path(
            reflike!("refs/remotes")
                .join(peer1.peer_id())
                .join(reflike!("heads"))
                .join(&default_branch),
        );

        proj.pull(peer1, peer2).await.unwrap();
        proj.pull(peer2, peer3).await.unwrap();

        let peer2_expected = peer2
            .using_storage({
                let urn = proj.project.urn();
                let expected_urn = expected_urn.clone();
                let remote = peer1.peer_id();
                let delegate = proj.owner.urn();
                move |storage| {
                    layout::References::new(
                        storage,
                        &urn,
                        remote,
                        Some(delegate),
                        Some((expected_urn, commit_id)),
                    )
                }
            })
            .await
            .unwrap()
            .unwrap();
        let peer3_expected = peer3
            .using_storage({
                let urn = proj.project.urn();
                let remote = peer1.peer_id();
                let delegate = proj.owner.urn();
                move |storage| {
                    layout::References::new(
                        storage,
                        &urn,
                        remote,
                        Some(delegate),
                        Some((expected_urn, commit_id)),
                    )
                }
            })
            .await
            .unwrap()
            .unwrap();

        let commits = peer2_expected
            .missing_commits()
            .map(|c| c.to_string())
            .collect::<Vec<_>>();
        assert!(commits.is_empty(), "peer 2 missing commits `{:?}`", commits);
        let rad_id = peer2_expected.rad_id();
        assert!(rad_id.exists, "peer 2 missing `{}`", rad_id);
        let rad_self = peer2_expected.rad_self();
        assert!(rad_self.exists, "peer 2 missing `{}`", rad_self);
        let ids = peer2_expected
            .missing_rad_ids()
            .map(|del| del.to_string())
            .collect::<Vec<_>>();
        assert!(ids.is_empty(), "peer 2 missing `{:?}`", ids);

        let commits = peer3_expected
            .missing_commits()
            .map(|c| c.to_string())
            .collect::<Vec<_>>();
        assert!(commits.is_empty(), "peer 3 missing commits `{:?}`", commits);
        let rad_id = peer3_expected.rad_id();
        assert!(rad_id.exists, "peer 3 missing `{}`", rad_id);
        let rad_self = peer3_expected.rad_self();
        assert!(rad_self.exists, "peer 3 missing `{}`", rad_self);
        let ids = peer3_expected
            .missing_rad_ids()
            .map(|del| del.to_string())
            .collect::<Vec<_>>();
        assert!(ids.is_empty(), "peer 3 missing `{:?}`", ids);
    })
}

/// `peer1` is a delegate of a project and tracks `peer2`.
/// When `peer3` replicates from `peer1` they should have references for `peer1`
/// and `peer2`, due to the tracking graph.
#[test]
fn threes_a_crowd() {
    logging::init();

    let net = testnet::run(config()).unwrap();
    net.enter(async {
        let peer1 = net.peers().index(0);
        let peer2 = net.peers().index(1);
        let peer3 = net.peers().index(2);

        for x in 0..=2 {
            info!("peer{}: {}", x + 1, net.peers().index(x).peer_id())
        }

        let proj = peer1
            .using_storage(TestProject::create)
            .await
            .unwrap()
            .unwrap();

        peer1
            .using_storage({
                let peer2_id = peer2.peer_id();
                let urn = proj.project.urn();
                move |storage| -> anyhow::Result<()> {
                    assert!(tracking::track(
                        storage,
                        &urn,
                        Some(peer2_id),
                        tracking::Config::default(),
                        tracking::policy::Track::Any,
                    )?
                    .is_ok());
                    Refs::update(storage, &urn)?;
                    Ok(())
                }
            })
            .await
            .unwrap()
            .unwrap();

        proj.pull(peer1, peer2).await.unwrap();
        proj.pull(peer2, peer1).await.unwrap();
        proj.pull(peer1, peer3).await.unwrap();

        // Has peer1 refs?
        let peer3_expected = peer3
            .using_storage({
                let urn = proj.project.urn();
                let delegate = proj.owner.urn();
                let remote = peer1.peer_id();
                move |storage| {
                    layout::References::new::<ext::Oid, _, _>(
                        storage,
                        &urn,
                        remote,
                        Some(delegate),
                        None,
                    )
                }
            })
            .await
            .unwrap()
            .unwrap();

        let rad_id = peer3_expected.rad_id();
        assert!(rad_id.exists, "peer 3 missing `{}`", rad_id);
        let rad_self = peer3_expected.rad_self();
        assert!(rad_self.exists, "peer 3 missing `{}`", rad_self);
        let ids = peer3_expected
            .missing_rad_ids()
            .map(|del| del.to_string())
            .collect::<Vec<_>>();
        assert!(ids.is_empty(), "peer 3 missing {:?}", ids);

        // Has peer2 refs?
        // Skipping rad/self since peer2 never creates a Person
        let peer3_expected = peer3
            .using_storage({
                let urn = proj.project.urn();
                let delegate = proj.owner.urn();
                let remote = peer2.peer_id();
                move |storage| {
                    layout::References::new::<ext::Oid, _, _>(
                        storage,
                        &urn,
                        remote,
                        Some(delegate),
                        None,
                    )
                }
            })
            .await
            .unwrap()
            .unwrap();

        let rad_id = peer3_expected.rad_id();
        assert!(rad_id.exists, "peer 3 missing `{}`", rad_id);
        let ids = peer3_expected
            .missing_rad_ids()
            .map(|del| del.to_string())
            .collect::<Vec<_>>();
        assert!(ids.is_empty(), "peer 3 missing {:?}", ids);
    })
}
