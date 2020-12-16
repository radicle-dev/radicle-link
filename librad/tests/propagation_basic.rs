// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#![feature(async_closure)]

use std::{
    convert::{TryFrom, TryInto},
    time::Duration,
};

use futures::{future, stream::StreamExt};
use tempfile::tempdir;

use librad::{
    git::{
        identities::{self, SomeIdentity},
        local::url::LocalUrl,
        replication,
        types::{remote, Flat, Force, GenericRef, Namespace, Reference, Refspec, Remote},
    },
    git_ext as ext,
    net::peer::{FetchInfo, Gossip, PeerEvent, Rev},
    reflike,
    refspec_pattern,
};

use librad_test::{
    git::create_commit,
    logging,
    rad::{
        identities::{create_test_project, TestProject},
        testnet,
    },
};

#[tokio::test]
async fn can_clone() {
    logging::init();

    const NUM_PEERS: usize = 2;

    let peers = testnet::setup(NUM_PEERS).await.unwrap();
    testnet::run_on_testnet(peers, NUM_PEERS, async move |mut apis| {
        let (peer1, _) = apis.pop().unwrap();
        let (peer2, _) = apis.pop().unwrap();

        let TestProject { project, owner } = peer1
            .with_storage(move |storage| create_test_project(&storage))
            .await
            .unwrap()
            .unwrap();

        peer2
            .with_storage(move |storage| {
                let urn = project.urn();
                replication::replicate(&storage, None, urn.clone(), peer1.peer_id(), None).unwrap();

                // check rad/self of peer1 exists
                assert!(
                    storage
                        .has_ref(&Reference::rad_self(Namespace::from(&urn), peer1.peer_id()))
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
    })
    .await;
}

#[tokio::test]
async fn can_clone_disconnected() {
    logging::init();

    const NUM_PEERS: usize = 2;

    let peers = testnet::setup_disconnected(NUM_PEERS).await.unwrap();
    testnet::run_on_testnet(peers, 0, async move |mut apis| {
        let (peer1, _) = apis.pop().unwrap();
        let (peer2, _) = apis.pop().unwrap();

        let TestProject { project, owner } = peer1
            .with_storage(move |storage| create_test_project(&storage))
            .await
            .unwrap()
            .unwrap();

        peer2
            .with_storage(move |storage| {
                let urn = project.urn();
                replication::replicate(
                    &storage,
                    None,
                    urn.clone(),
                    peer1.peer_id(),
                    peer1.listen_addrs(),
                )
                .unwrap();

                // check rad/self of peer1 exists
                assert!(
                    storage
                        .has_ref(&Reference::rad_self(Namespace::from(&urn), peer1.peer_id()))
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
    })
    .await;
}

#[tokio::test(core_threads = 2)]
async fn fetches_on_gossip_notify() {
    logging::init();

    const NUM_PEERS: usize = 2;

    let peers = testnet::setup(NUM_PEERS).await.unwrap();
    testnet::run_on_testnet(peers, NUM_PEERS, async move |mut apis| {
        let (peer1, _) = apis.pop().unwrap();
        let (peer2, _) = apis.pop().unwrap();

        let TestProject { project, owner } = peer1
            .with_storage(move |storage| create_test_project(&storage))
            .await
            .unwrap()
            .unwrap();
        peer2
            .with_storage({
                let urn = project.urn();
                let peer_id = peer1.peer_id();
                move |storage| replication::replicate(&storage, None, urn, peer_id, None)
            })
            .await
            .unwrap()
            .expect("should be able to replicate");

        let peer2_events = peer2.subscribe().await;

        let mastor = reflike!("refs/heads/master");
        // Check out a working copy on peer1, add a commit, and push it
        let commit_id = {
            let tmp = tempdir().unwrap();
            let repo = git2::Repository::init(tmp.path()).unwrap();
            let url = LocalUrl::from(project.urn());

            let mut remote = Remote::rad_remote(
                url,
                Refspec {
                    src: Reference::heads(Namespace::from(project.urn()), peer1.peer_id()),
                    dst: GenericRef::heads(
                        Flat,
                        ext::RefLike::try_from(format!(
                            "{}@{}",
                            owner.subject().name,
                            peer1.peer_id(),
                        ))
                        .unwrap(),
                    ),
                    force: Force::True,
                }
                .into_fetchspec(),
            );

            let oid = create_commit(&repo, mastor.clone()).unwrap();
            remote
                .push(
                    peer1.clone(),
                    &repo,
                    remote::LocalPushspec::Matching {
                        pattern: refspec_pattern!("refs/heads/*"),
                        force: Force::True,
                    },
                )
                .unwrap()
                .for_each(drop);
            peer1
                .protocol()
                .announce(Gossip {
                    origin: None,
                    urn: project.urn().with_path(mastor.clone()),
                    rev: Some(Rev::Git(oid)),
                })
                .await;

            oid
        };

        // Wait for peer2 to receive the gossip announcement
        {
            let peer1_id = peer1.peer_id();
            tokio::time::timeout(
                Duration::from_secs(5),
                peer2_events
                    .filter(|event| match event {
                        PeerEvent::GossipFetch(FetchInfo { provider, .. }) => {
                            future::ready(*provider == peer1_id)
                        },
                    })
                    .map(|_| ())
                    .next(),
            )
            .await
            .unwrap();
        }

        // Check that peer2 has fetched the update
        let peer2_has_commit = peer2
            .with_storage(move |storage| {
                storage.has_commit(
                    &project.urn().with_path(
                        reflike!("refs/remotes")
                            .join(peer1.peer_id())
                            .join(mastor.strip_prefix("refs").unwrap()),
                    ),
                    Box::new(commit_id),
                )
            })
            .await
            .unwrap()
            .unwrap();
        assert!(peer2_has_commit);
    })
    .await;
}

// FIXME(kim): does this belong here?
#[tokio::test]
async fn list_identities_returns_only_local_projects() {
    logging::init();

    const NUM_PEERS: usize = 3;

    let peers = testnet::setup(NUM_PEERS).await.unwrap();
    testnet::run_on_testnet(peers, NUM_PEERS, async move |mut apis| {
        let (peer1, _) = apis.pop().unwrap();
        let (peer2, _) = apis.pop().unwrap();
        let (peer3, _) = apis.pop().unwrap();

        let TestProject { project, .. } = peer1
            .with_storage(move |storage| create_test_project(&storage))
            .await
            .unwrap()
            .unwrap();

        peer2
            .with_storage({
                let urn = project.urn();
                let remote_peer = peer1.peer_id();
                move |storage| replication::replicate(&storage, None, urn, remote_peer, None)
            })
            .await
            .unwrap()
            .unwrap();

        let all_identities = peer3
            .with_storage({
                let urn = project.urn();
                let remote_peer = peer2.peer_id();
                move |storage| -> Result<Vec<SomeIdentity>, anyhow::Error> {
                    replication::replicate(&storage, None, urn, remote_peer, None)?;
                    Ok(identities::any::list(&storage)?.collect::<Result<Vec<_>, _>>()?)
                }
            })
            .await
            .unwrap()
            .unwrap();

        assert_eq!(2, all_identities.len());
    })
    .await;
}

/// Given that a) a peer 1 holds a given URN and b) that same peer is a seed of
/// a peer 2, verify that requesting peer 2 for providers for said URN returns
/// peer 1.
///
/// Following that, verify that cloning from the returned PeerId means we have
/// the URN in our monorepo.
#[tokio::test]
async fn ask_and_clone() {
    logging::init();
    const NUM_PEERS: usize = 2;
    let peers = testnet::setup(NUM_PEERS).await.unwrap();

    testnet::run_on_testnet(peers, NUM_PEERS, async move |mut apis| {
        let (peer1, _) = apis.pop().unwrap();

        let TestProject { project, .. } = peer1
            .with_storage(move |storage| create_test_project(&storage))
            .await
            .unwrap()
            .unwrap();

        let (peer2, _) = apis.pop().unwrap();
        let res = peer2
            .providers(project.urn(), Duration::from_secs(5))
            .await
            .next()
            .await;

        let remote_peer = match res {
            Some(peer_info) => peer_info.peer_id,
            None => panic!("Expected to have obtained peer1 but got None instead"),
        };

        let peer2_has_urn = async || {
            peer2
                .with_storage({
                    let urn = project.urn();
                    move |storage| storage.has_urn(&urn)
                })
                .await
                .unwrap()
                .unwrap()
        };

        assert_eq!(
            false,
            peer2_has_urn().await,
            "expected peer2 to not have URN {} yet",
            project.urn()
        );

        peer2
            .with_storage({
                let urn = project.urn();
                move |storage| replication::replicate(&storage, None, urn, remote_peer, None)
            })
            .await
            .unwrap()
            .unwrap();

        assert_eq!(
            true,
            peer2_has_urn().await,
            "expected peer2 to have URN {}",
            project.urn()
        )
    })
    .await;
}

#[tokio::test(core_threads = 2)]
async fn menage_a_troi() {
    logging::init();

    const NUM_PEERS: usize = 3;

    let peers = testnet::setup(NUM_PEERS).await.unwrap();
    testnet::run_on_testnet(peers, NUM_PEERS, async move |mut apis| {
        let (peer1, _) = apis.pop().unwrap();
        let (peer2, _) = apis.pop().unwrap();
        let (peer3, _) = apis.pop().unwrap();

        let TestProject { project, owner } = peer1
            .with_storage(move |storage| create_test_project(&storage))
            .await
            .unwrap()
            .unwrap();
        let default_branch: ext::RefLike = project
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
        let commit_id = {
            // Perform commit and push to working copy on peer1
            let repo = git2::Repository::init(tmp.path().join("peer1")).unwrap();
            let url = LocalUrl::from(project.urn());
            let heads = Reference::heads(Namespace::from(project.urn()), Some(peer1.peer_id()));
            let remotes = GenericRef::heads(
                Flat,
                ext::RefLike::try_from(format!("{}@{}", owner.subject().name, peer1.peer_id()))
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
                    peer1.clone(),
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
        };

        let expected_urn = project.urn().with_path(
            reflike!("refs/remotes")
                .join(peer1.peer_id())
                .join(reflike!("heads"))
                .join(&default_branch),
        );

        struct ExpectedReferences {
            has_commit: bool,
            has_rad_id: bool,
            has_rad_self: bool,
        }

        let peer2_expected = peer2
            .with_storage({
                let remote_peer = peer1.peer_id();
                let urn = expected_urn.clone();
                let rad_self = Reference::rad_self(Namespace::from(urn.clone()), peer1.peer_id());
                let rad_id = Reference::rad_id(Namespace::from(urn.clone())).with_remote(peer1.peer_id());
                let addrs = peer1.listen_addrs().collect::<Vec<_>>();
                move |storage| -> Result<ExpectedReferences, anyhow::Error> {
                    replication::replicate(&storage, None, urn.clone(), remote_peer, addrs)?;
                    Ok(ExpectedReferences {
                        has_commit: storage.has_commit(&urn, Box::new(commit_id))?,
                        has_rad_id: storage.has_ref(&rad_self)?,
                        has_rad_self: storage.has_ref(&rad_id)?,
                    })
                }
            })
            .await
            .unwrap()
            .unwrap();
        let peer3_expected = peer3
            .with_storage({
                let remote_peer = peer2.peer_id();
                let urn = expected_urn.clone();
                let rad_self = Reference::rad_self(Namespace::from(urn.clone()), peer1.peer_id());
                let rad_id = Reference::rad_id(Namespace::from(urn.clone())).with_remote(peer1.peer_id());
                let addrs = peer2.listen_addrs().collect::<Vec<_>>();
                move |storage| -> Result<ExpectedReferences, anyhow::Error> {
                    replication::replicate(&storage, None, urn.clone(), remote_peer, addrs)?;
                    Ok(ExpectedReferences {
                        has_commit: storage.has_commit(&urn, Box::new(commit_id))?,
                        has_rad_id: storage.has_ref(&rad_self)?,
                        has_rad_self: storage.has_ref(&rad_id)?,
                    })
                }
            })
            .await
            .unwrap()
            .unwrap();

        assert!(
            peer2_expected.has_commit,
            format!("peer 2 missing commit `{}@{}`", expected_urn, commit_id)
        );
        assert!(
            peer2_expected.has_rad_id,
            format!("peer 2 missing `rad/id`")
        );
        assert!(
            peer2_expected.has_rad_self,
            format!("peer 2 missing `rad/self``")
        );

        assert!(
            peer3_expected.has_commit,
            format!("peer 3 missing commit `{}@{}`", expected_urn, commit_id)
        );
        assert!(
            peer3_expected.has_rad_id,
            format!("peer 3 missing `rad/id`")
        );
        assert!(
            peer3_expected.has_rad_self,
            format!("peer 3 missing `rad/self``")
        );
    })
    .await;
}
