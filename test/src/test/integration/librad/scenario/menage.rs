// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    convert::{TryFrom, TryInto},
    fmt::Debug,
    ops::Index as _,
};

use crate::{
    git::create_commit,
    logging,
    rad::{identities::TestProject, testnet},
};
use blocking::unblock;
use librad::{
    self,
    git::{
        local::url::LocalUrl,
        storage::{ReadOnlyStorage as _, Storage},
        tracking,
        types::{remote, Flat, Force, GenericRef, Namespace, Reference, Refspec, Remote},
        Urn,
    },
    git_ext as ext,
    peer::PeerId,
    reflike,
    refspec_pattern,
};

fn config() -> testnet::Config {
    testnet::Config {
        num_peers: nonzero!(3usize),
        min_connected: 3,
        bootstrap: testnet::Bootstrap::from_env(),
    }
}

struct ExpectedReferences {
    has_commit: bool,
    has_rad_id: bool,
    has_rad_self: bool,
    has_rad_ids: bool,
}

impl ExpectedReferences {
    fn new<Oid>(
        storage: &Storage,
        urn: &Urn,
        remote: PeerId,
        delegate: Urn,
        commit: Option<Oid>,
    ) -> Result<Self, anyhow::Error>
    where
        Oid: AsRef<git2::Oid> + Debug,
    {
        let rad_self = Reference::rad_self(Namespace::from(urn.clone()), remote);
        let rad_id = Reference::rad_id(Namespace::from(urn.clone())).with_remote(remote);
        let rad_ids =
            Reference::rad_delegate(Namespace::from(urn.clone()), &delegate).with_remote(remote);

        Ok(ExpectedReferences {
            has_commit: commit.map_or(Ok(true), |commit| storage.has_commit(&urn, commit))?,
            has_rad_id: storage.has_ref(&rad_id)?,
            has_rad_self: storage.has_ref(&rad_self)?,
            has_rad_ids: storage.has_ref(&rad_ids)?,
        })
    }
}

#[test]
#[ignore]
fn a_trois() {
    logging::init();

    let net = testnet::run(config()).unwrap();
    net.enter(async {
        let peer1 = net.peers().index(0);
        let peer2 = net.peers().index(1);
        let peer3 = net.peers().index(2);

        let proj = peer1
            .using_storage(move |storage| TestProject::create(&storage))
            .await
            .unwrap()
            .unwrap();
        let default_branch: ext::RefLike = proj
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
        let tmp = tempfile::tempdir().unwrap();
        let commit_id = unblock({
            let urn = proj.project.urn();
            let owner_subject = proj.owner.subject().clone();
            let default_branch = default_branch.clone();
            let peer1 = (*peer1).clone();
            move || {
                // Perform commit and push to working copy on peer1
                let repo = git2::Repository::init(tmp.path().join("peer1")).unwrap();
                let url = LocalUrl::from(urn.clone());
                let heads = Reference::heads(Namespace::from(urn), Some(peer1.peer_id()));
                let remotes = GenericRef::heads(
                    Flat,
                    ext::RefLike::try_from(format!("{}@{}", owner_subject.name, peer1.peer_id()))
                        .unwrap(),
                );
                let mastor = reflike!("refs/heads").join(&default_branch);
                let mut remote = Remote::rad_remote(
                    url,
                    Refspec {
                        src: &remotes,
                        dst: &heads,
                        force: Force::True,
                    },
                );
                let oid = create_commit(&repo, mastor).unwrap();
                let updated = remote
                    .push(
                        peer1,
                        &repo,
                        remote::LocalPushspec::Matching {
                            pattern: refspec_pattern!("refs/heads/*"),
                            force: Force::True,
                        },
                    )
                    .unwrap()
                    .collect::<Vec<_>>();
                tracing::debug!("push updated refs: {:?}", updated);

                oid
            }
        })
        .await;

        let expected_urn = proj.project.urn().with_path(
            reflike!("refs/remotes")
                .join(peer1.peer_id())
                .join(reflike!("heads"))
                .join(&default_branch),
        );

        proj.pull(peer1, peer2).await.ok().unwrap();
        proj.pull(peer2, peer3).await.ok().unwrap();

        let peer2_expected = peer2
            .using_storage({
                let urn = expected_urn.clone();
                let remote = peer1.peer_id();
                let delegate = proj.owner.urn();
                move |storage| {
                    ExpectedReferences::new(
                        storage,
                        &urn,
                        remote,
                        delegate,
                        Some(Box::new(commit_id)),
                    )
                }
            })
            .await
            .unwrap()
            .unwrap();
        let peer3_expected = peer3
            .using_storage({
                let urn = expected_urn.clone();
                let remote = peer1.peer_id();
                let delegate = proj.owner.urn();
                move |storage| {
                    ExpectedReferences::new(
                        storage,
                        &urn,
                        remote,
                        delegate,
                        Some(Box::new(commit_id)),
                    )
                }
            })
            .await
            .unwrap()
            .unwrap();
        assert!(
            peer2_expected.has_commit,
            "peer 2 missing commit `{}@{}`",
            expected_urn, commit_id
        );
        assert!(peer2_expected.has_rad_id, "peer 2 missing `rad/id`");
        assert!(peer2_expected.has_rad_self, "peer 2 missing `rad/self``");
        assert!(
            peer2_expected.has_rad_ids,
            "peer 2 missing `rad/ids/<delegate>`"
        );

        assert!(
            peer3_expected.has_commit,
            "peer 3 missing commit `{}@{}`",
            expected_urn, commit_id
        );
        assert!(peer3_expected.has_rad_id, "peer 3 missing `rad/id`");
        assert!(peer3_expected.has_rad_self, "peer 3 missing `rad/self``");
        assert!(
            peer3_expected.has_rad_ids,
            "peer 3 missing `rad/ids/<delegate>`"
        );
    })
}

/// `peer1` is a delegate of a project and tracks `peer2`.
/// When `peer3` replicates from `peer1` they should have references for `peer1`
/// and `peer2`, due to the tracking graph.
#[test]
#[ignore]
fn threes_a_crowd() {
    logging::init();

    let net = testnet::run(config()).unwrap();
    net.enter(async {
        let peer1 = net.peers().index(0);
        let peer2 = net.peers().index(1);
        let peer3 = net.peers().index(2);

        let proj = peer1
            .using_storage(move |storage| TestProject::create(&storage))
            .await
            .unwrap()
            .unwrap();

        peer1
            .using_storage({
                let peer_id = peer2.peer_id();
                let urn = proj.project.urn();
                move |storage| tracking::track(storage, &urn, peer_id)
            })
            .await
            .unwrap()
            .unwrap();
        proj.pull(peer1, peer2).await.ok().unwrap();
        proj.pull(peer2, peer1).await.ok().unwrap();
        proj.pull(peer1, peer3).await.ok().unwrap();

        // Has peer1 refs?
        let peer3_expected = peer3
            .using_storage({
                let urn = proj.project.urn();
                let delegate = proj.owner.urn();
                let remote = peer1.peer_id();
                move |storage| {
                    ExpectedReferences::new::<ext::Oid>(storage, &urn, remote, delegate, None)
                }
            })
            .await
            .unwrap()
            .unwrap();

        assert!(peer3_expected.has_rad_id, "peer 3 missing `rad/id`");
        assert!(peer3_expected.has_rad_self, "peer 3 missing `rad/self``");
        assert!(
            peer3_expected.has_rad_ids,
            "peer 3 missing `rad/ids/<delegate>`"
        );

        // Has peer2 refs?
        // Skipping rad/self since peer2 never creates a Person
        let peer3_expected = peer3
            .using_storage({
                let urn = proj.project.urn();
                let delegate = proj.owner.urn();
                let remote = peer2.peer_id();
                move |storage| {
                    ExpectedReferences::new::<ext::Oid>(storage, &urn, remote, delegate, None)
                }
            })
            .await
            .unwrap()
            .unwrap();

        assert!(peer3_expected.has_rad_id, "peer 3 missing `rad/id`");
        assert!(
            peer3_expected.has_rad_ids,
            "peer 3 missing `rad/ids/<delegate>`"
        );
    })
}
