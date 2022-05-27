// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom, ops::Index as _};

use tempfile::tempdir;

use git_ref_format::{lit, name, Namespaced, Qualified, RefString};
use it_helpers::{
    fixed::{TestPerson, TestProject},
    testnet::{self, RunningTestPeer},
    working_copy::{WorkingCopy, WorkingRemote as Remote},
};
use librad::git::{
    identities::{self, local, project::heads},
    storage::ReadOnlyStorage,
};
use link_identities::payload;
use test_helpers::logging;

fn config() -> testnet::Config {
    testnet::Config {
        num_peers: nonzero!(2usize),
        min_connected: 2,
        bootstrap: testnet::Bootstrap::from_env(),
    }
}

/// This test checks that the logic of `librad::git::identities::project::heads`
/// is correct. To do this we need to set up various scenarios where the
/// delegates of a project agree or disagree on the default branch of a project.
#[test]
fn default_branch_head() {
    logging::init();

    let net = testnet::run(config()).unwrap();
    net.enter(async {
        // Setup  a testnet with two peers
        let peer1 = net.peers().index(0);
        let peer2 = net.peers().index(1);

        // Create an identity on peer2
        let peer2_id = peer2
            .using_storage::<_, anyhow::Result<TestPerson>>(|s| {
                let person = TestPerson::create(s)?;
                let local = local::load(s, person.owner.urn()).unwrap();
                s.config()?.set_user(local)?;
                Ok(person)
            })
            .await
            .unwrap()
            .unwrap();

        peer2_id.pull(peer2, peer1).await.unwrap();

        // Create a project on peer1
        let proj = peer1
            .using_storage(|s| {
                TestProject::create_with_payload(
                    s,
                    payload::Project {
                        name: "venus".into(),
                        description: None,
                        default_branch: Some(name::MASTER.to_string().into()),
                    },
                )
            })
            .await
            .unwrap()
            .unwrap();

        // Add peer2 as a maintainer
        proj.maintainers(peer1)
            .add(&peer2_id, peer2)
            .setup()
            .await
            .unwrap();

        //// Okay, now we have a running testnet with two Peers, each of which has a
        //// `Person` who is a delegate on the `TestProject`

        // Create a basic history which contains two commits, one from peer1 and one
        // from peer2
        //
        // * Create a commit in peer 1
        // * pull to peer2
        // * create a new commit on top of the original commit in peer2
        // * pull peer2 back to peer1
        // * in peer1 fast forward and push
        //
        // At this point both peers should have a history like
        //
        //     peer1 commit
        //           ↓
        //     peer2 commit
        let tmp = tempdir().unwrap();
        let tip = {
            let mut working_copy1 =
                WorkingCopy::new(&proj, tmp.path().join("peer1"), peer1).unwrap();
            let mut working_copy2 =
                WorkingCopy::new(&proj, tmp.path().join("peer2"), peer2).unwrap();

            let mastor = Qualified::from(lit::refs_heads(name::MASTER));
            working_copy1
                .commit("peer 1 initial", mastor.clone())
                .unwrap();
            working_copy1.push().unwrap();
            proj.pull(peer1, peer2).await.unwrap();

            working_copy2.fetch(Remote::Peer(peer1.peer_id())).unwrap();
            working_copy2
                .create_remote_tracking_branch(Remote::Peer(peer1.peer_id()), name::MASTER)
                .unwrap();
            let tip = working_copy2
                .commit("peer 2 initial", mastor.clone())
                .unwrap();
            working_copy2.push().unwrap();
            proj.pull(peer2, peer1).await.unwrap();

            working_copy1.fetch(Remote::Peer(peer2.peer_id())).unwrap();
            working_copy1
                .fast_forward_to(Remote::Peer(peer2.peer_id()), name::MASTER)
                .unwrap();
            working_copy1.push().unwrap();
            tip
        };

        let default_branch = branch_head(peer1, &proj).await.unwrap();
        // The two peers should have the same view of the default branch
        assert_eq!(
            default_branch,
            identities::project::heads::DefaultBranchHead::Head {
                target: tip,
                branch: name::MASTER.to_owned(),
            }
        );

        // now update peer1 and push to peer 1s monorepo, we should still get the old
        // tip because peer2 is behind
        let tmp = tempdir().unwrap();
        let new_tip = {
            let mut working_copy1 =
                WorkingCopy::new(&proj, tmp.path().join("peer1"), peer1).unwrap();
            working_copy1
                .create_remote_tracking_branch(Remote::Rad, name::MASTER)
                .unwrap();

            let mastor = Qualified::from(lit::refs_heads(name::MASTER));
            let tip = working_copy1
                .commit("peer 1 update", mastor.clone())
                .unwrap();
            working_copy1.push().unwrap();

            tip
        };

        let default_branch_peer1 = branch_head(peer1, &proj).await.unwrap();
        assert_eq!(
            default_branch_peer1,
            identities::project::heads::DefaultBranchHead::Head {
                target: tip,
                branch: name::MASTER.to_owned(),
            }
        );

        // fast forward peer2 and pull the update back into peer1
        let tmp = tempdir().unwrap();
        proj.pull(peer1, peer2).await.unwrap();
        {
            let mut working_copy2 =
                WorkingCopy::new(&proj, tmp.path().join("peer2"), peer2).unwrap();
            working_copy2
                .create_remote_tracking_branch(Remote::Rad, name::MASTER)
                .unwrap();

            working_copy2.fetch(Remote::Peer(peer1.peer_id())).unwrap();
            working_copy2
                .fast_forward_to(Remote::Peer(peer1.peer_id()), name::MASTER)
                .unwrap();
            working_copy2.push().unwrap();
        }
        proj.pull(peer2, peer1).await.unwrap();

        // Now we should be pointing at the latest tip because both peer1 and peer2
        // agree
        let default_branch_peer1 = branch_head(peer1, &proj).await.unwrap();
        assert_eq!(
            default_branch_peer1,
            identities::project::heads::DefaultBranchHead::Head {
                target: new_tip,
                branch: name::MASTER.to_owned(),
            }
        );

        // now create an alternate commit on peer2 and sync with peer1, on peer1 we
        // should get a fork
        let tmp = tempdir().unwrap();
        let forked_tip = {
            let mut working_copy2 =
                WorkingCopy::new(&proj, tmp.path().join("peer2"), peer2).unwrap();

            let mastor = Qualified::from(lit::refs_heads(name::MASTER));
            let forked_tip = working_copy2.commit("peer 2 fork", mastor.clone()).unwrap();
            working_copy2.push().unwrap();

            forked_tip
        };
        proj.pull(peer2, peer1).await.unwrap();

        let default_branch_peer1 = branch_head(peer1, &proj).await.unwrap();
        assert_eq!(
            default_branch_peer1,
            identities::project::heads::DefaultBranchHead::Forked(
                vec![
                    identities::project::heads::Fork {
                        peers: std::iter::once(peer1.peer_id()).collect(),
                        tip: new_tip,
                    },
                    identities::project::heads::Fork {
                        peers: std::iter::once(peer2.peer_id()).collect(),
                        tip: forked_tip,
                    }
                ]
                .into_iter()
                .collect()
            )
        );

        // now merge the fork into peer1
        let tmp = tempdir().unwrap();
        let fixed_tip = {
            let mut working_copy1 =
                WorkingCopy::new(&proj, tmp.path().join("peer1"), peer1).unwrap();
            working_copy1.fetch(Remote::Peer(peer2.peer_id())).unwrap();
            working_copy1
                .create_remote_tracking_branch(Remote::Peer(peer2.peer_id()), name::MASTER)
                .unwrap();

            working_copy1.fetch(Remote::Peer(peer2.peer_id())).unwrap();
            let tip = working_copy1
                .merge_remote(peer2.peer_id(), name::MASTER)
                .unwrap();
            working_copy1.push().unwrap();
            tip
        };

        // pull the merge into peer2
        proj.pull(peer1, peer2).await.unwrap();
        {
            let mut working_copy2 =
                WorkingCopy::new(&proj, tmp.path().join("peer2"), peer2).unwrap();
            working_copy2
                .create_remote_tracking_branch(Remote::Rad, name::MASTER)
                .unwrap();

            working_copy2.fetch(Remote::Peer(peer1.peer_id())).unwrap();
            working_copy2
                .fast_forward_to(Remote::Peer(peer1.peer_id()), name::MASTER)
                .unwrap();
            working_copy2.push().unwrap();
        }
        proj.pull(peer2, peer1).await.unwrap();

        let default_branch_peer1 = branch_head(peer1, &proj).await.unwrap();
        assert_eq!(
            default_branch_peer1,
            identities::project::heads::DefaultBranchHead::Head {
                target: fixed_tip,
                branch: name::MASTER.to_owned(),
            }
        );

        // now set the head in the monorepo and check that the HEAD reference exists
        let updated_tip = peer1
            .using_storage::<_, anyhow::Result<_>>({
                let urn = proj.project.urn();
                move |s| {
                    let vp = identities::project::verify(s, &urn)?.ok_or_else(|| {
                        anyhow::anyhow!("failed to get project for default branch")
                    })?;
                    identities::project::heads::set_default_head(s, vp).map_err(anyhow::Error::from)
                }
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated_tip, fixed_tip);

        let head_ref = RefString::try_from(format!(
            "refs/namespaces/{}/refs/HEAD",
            proj.project.urn().encode_id()
        ))
        .unwrap();
        let master_ref = Namespaced::from(lit::refs_namespaces(
            &proj.project.urn(),
            Qualified::from(lit::refs_heads(name::MASTER)),
        ));
        let (master_oid, head_target) = peer1
            .using_storage::<_, anyhow::Result<_>>({
                let master_ref = master_ref.clone();
                move |s| {
                    let master_oid = s
                        .reference(&master_ref.into_qualified().into_refstring())?
                        .ok_or_else(|| anyhow::anyhow!("master ref not found"))?
                        .peel_to_commit()?
                        .id();
                    let head_target = s
                        .reference(&head_ref)?
                        .ok_or_else(|| anyhow::anyhow!("head ref not found"))?
                        .symbolic_target()
                        .map(|s| s.to_string());
                    Ok((master_oid, head_target))
                }
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(master_oid, updated_tip);
        assert_eq!(head_target, Some(master_ref.to_string()));
    });
}

async fn branch_head(
    peer: &RunningTestPeer,
    proj: &TestProject,
) -> anyhow::Result<heads::DefaultBranchHead> {
    peer.using_storage::<_, anyhow::Result<_>>({
        let urn = proj.project.urn();
        move |s| {
            let vp = identities::project::verify(s, &urn)?
                .ok_or_else(|| anyhow::anyhow!("failed to get project for default branch"))?;
            heads::default_branch_head(s, vp).map_err(anyhow::Error::from)
        }
    })
    .await?
}
