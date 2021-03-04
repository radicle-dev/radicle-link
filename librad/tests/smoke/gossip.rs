// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{ops::Deref, time::Duration};

use futures::StreamExt as _;
use librad::{
    git::{
        local::url::LocalUrl,
        types::{remote, Fetchspec, Force, Reference, Remote},
        Urn,
    },
    net::{
        peer::Peer,
        protocol::{
            event::{self, upstream::predicate::gossip_from},
            gossip::{self, Rev},
        },
    },
    reflike,
    refspec_pattern,
    signer::Signer,
};
use librad_test::{
    git::create_commit,
    logging,
    rad::{identities::TestProject, testnet},
};
use tempfile::tempdir;

const NUM_PEERS: usize = 2;

/// Given two connected peers.
/// Then create a project for peer1.
/// Then have peer2 track peer1’s project.
/// Then create a a commit, a branch and a tag for that project and push it to
/// peer1. Then wait for peer2 to receive announcements for the project.
/// Assert that peer2’s monorepo contains the comit, the branch and the tag from
/// peer1.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fetches_on_gossip_notify() {
    logging::init();

    let peers = testnet::setup(NUM_PEERS).await.unwrap();
    testnet::run_on_testnet(peers, NUM_PEERS, |mut peers| async move {
        let peer1 = peers.pop().unwrap();
        let peer2 = peers.pop().unwrap();

        let proj = peer1
            .using_storage(move |storage| TestProject::create(&storage))
            .await
            .unwrap()
            .unwrap();
        proj.pull(&peer1, &peer2).await.ok().unwrap();

        let TestProject { project, owner: _ } = proj;
        let peer2_events = peer2.subscribe();

        let mastor = reflike!("refs/heads/master");
        let project_repo_path = tempdir().unwrap();
        let repo = git2::Repository::init(&project_repo_path).unwrap();
        let url = LocalUrl::from(project.urn());

        let mut remote = Remote::rad_remote::<_, Fetchspec>(url, None);

        let commit_id = create_commit(&repo, mastor.clone()).unwrap();
        let commit = repo.find_object(commit_id, None).unwrap();

        let author = git2::Signature::now("The Animal", "animal@muppets.com").unwrap();
        let tag_id = repo
            .tag("MY-TAG", &commit, &author, "MESSAGE", false)
            .unwrap();

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
        remote
            .push(
                peer1.clone(),
                &repo,
                remote::LocalPushspec::Matching {
                    pattern: refspec_pattern!("refs/tags/*"),
                    force: Force::True,
                },
            )
            .unwrap()
            .for_each(drop);

        peer1
            .announce(gossip::Payload {
                origin: None,
                urn: project.urn().with_path(mastor.clone()),
                rev: Some(Rev::Git(commit_id)),
            })
            .unwrap();
        peer1
            .announce(gossip::Payload {
                origin: None,
                urn: project.urn().with_path(reflike!("refs/tags/MY-TAG")),
                rev: Some(Rev::Git(tag_id)),
            })
            .unwrap();

        // Wait for peer2 to receive the gossip announcement
        event::upstream::expect(
            peer2_events.boxed(),
            gossip_from(peer1.peer_id()),
            Duration::from_secs(5),
        )
        .await
        .unwrap();

        let commit_urn = project.urn().with_path(
            reflike!("refs/remotes")
                .join(peer1.peer_id())
                .join(mastor.strip_prefix("refs").unwrap()),
        );
        let storage = peer2.storage().await.unwrap();
        let peer2_has_commit = storage
            .as_ref()
            .has_commit(&commit_urn, Box::new(commit_id))
            .unwrap();
        assert!(peer2_has_commit);

        let remote_tag_ref = Reference::tag(
            Some(project.urn().into()),
            peer1.peer_id(),
            reflike!("MY-TAG"),
        );

        let tag_ref = storage
            .as_ref()
            .reference(&remote_tag_ref)
            .unwrap()
            .unwrap();
        assert_eq!(tag_ref.target(), Some(tag_id));
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

    let peers = testnet::setup(NUM_PEERS).await.unwrap();
    testnet::run_on_testnet(peers, NUM_PEERS, |mut peers| async move {
        let peer1 = peers.pop().unwrap();
        let peer2 = peers.pop().unwrap();

        let proj = peer1
            .using_storage(move |storage| TestProject::create(&storage))
            .await
            .unwrap()
            .unwrap();
        let project_urn = proj.project.urn();

        let provider = peer2
            .providers(project_urn.clone(), Duration::from_secs(5))
            .next()
            .await;
        assert_eq!(
            Some(peer1.peer_id()),
            provider.map(|info| info.peer_id),
            "Expected to have obtained peer1 as provider, but got nothing instead"
        );

        async fn has_urn<P, S>(peer: &P, urn: Urn) -> bool
        where
            P: Deref<Target = Peer<S>>,
            S: Signer + Clone,
        {
            peer.using_storage(move |storage| storage.has_urn(&urn))
                .await
                .unwrap()
                .unwrap()
        }

        assert_eq!(
            false,
            has_urn(&peer2, project_urn.clone()).await,
            "expected peer2 to not have URN {} yet",
            project_urn
        );

        proj.pull(&peer1, &peer2).await.ok().unwrap();

        assert_eq!(
            true,
            has_urn(&peer2, project_urn.clone()).await,
            "expected peer2 to have URN {}",
            project_urn
        )
    })
    .await;
}
