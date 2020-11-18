// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

#![feature(async_closure)]

use std::{
    convert::{identity, TryFrom},
    fmt::Debug,
    path::Path,
    time::Duration,
};

use futures::{
    future,
    stream::{Stream, StreamExt},
};
use tempfile::tempdir;

use librad::{
    git::{
        identities::{self, Project, User},
        include,
        local::url::LocalUrl,
        replication,
        tracking,
        types::{
            remote::{LocalFetchspec, LocalPushspec},
            Flat,
            Force,
            GenericRef,
            Namespace,
            Reference,
            Refspec,
            Remote,
        },
        Urn,
    },
    git_ext as ext,
    net::peer::{FetchInfo, Gossip, PeerApi, PeerEvent, Rev},
    peer::PeerId,
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

/// This integration test is to ensure that we can setup a working copy that can
/// fetch changes. The breakdown of the test into substeps is:
///
/// 1. Two peers are setup: peer1 and peer2.
/// 2. peer1 creates a project in their monorepo
/// 3. peer2 clones it
/// 4. peer1 creates a working copy and commits changes to it
/// 5. peer2 receives the changes via an announcement
/// 6. peer2 decides to create a working copy
/// 7. peer2 creates an include file, based of the tracked users of the project
/// i.e. peer1 8. peer2 includes this file in their working copy's config
/// 9. peer2 fetches in the working copy and sees the commit
#[tokio::test(core_threads = 2)]
async fn can_fetch() {
    logging::init();

    const NUM_PEERS: usize = 2;

    let peers = testnet::setup(NUM_PEERS).await.unwrap();
    testnet::run_on_testnet(peers, NUM_PEERS, async move |mut apis| {
        let (peer1, _) = apis.pop().unwrap();
        let (peer2, _) = apis.pop().unwrap();

        let peer2_events = peer2.subscribe().await;

        let TestProject { project, owner } = peer1
            .with_storage(move |store| create_test_project(&store))
            .await
            .unwrap()
            .unwrap();

        let tracked_users = {
            let urn = project.urn();
            let peer1_id = peer1.peer_id();
            peer2
                .with_storage(move |store| {
                    replication::replicate(&store, None, urn.clone(), peer1_id, None).unwrap();
                    tracking::tracked(&store, &urn)
                        .unwrap()
                        .map(|peer| {
                            let self_ref = Reference::rad_self(Namespace::from(&urn), peer);
                            let user = identities::user::get(
                                &store,
                                &Urn::try_from(self_ref).expect("namespace is set"),
                            )
                            .unwrap()
                            .expect("tracked user should exist");
                            (user, peer)
                        })
                        .collect::<Vec<(User, PeerId)>>()
                })
                .await
                .unwrap()
        };
        assert!(!tracked_users.is_empty());

        let tmp = tempdir().unwrap();
        {
            let commit_id = commit_and_push(tmp.path().join("peer1"), &peer1, &owner, &project)
                .await
                .unwrap();
            wait_for_event(peer2_events, peer1.peer_id()).await;
            let peer2_repo = create_working_copy(
                tmp.path().join("peer2"),
                tmp.path().to_path_buf(),
                &peer2,
                &project,
                tracked_users,
            )
            .unwrap();
            assert!(peer2_repo.find_commit(commit_id).is_ok());
        }
    })
    .await;
}

// Perform commit and push to working copy on peer1
#[tracing::instrument(skip(peer), err)]
async fn commit_and_push<P>(
    repo_path: P,
    peer: &PeerApi,
    owner: &User,
    project: &Project,
) -> Result<git2::Oid, anyhow::Error>
where
    P: AsRef<Path> + Debug,
{
    let repo = git2::Repository::init(repo_path)?;
    let url = LocalUrl::from(project.urn());

    let fetchspec = Refspec {
        src: Reference::heads(Namespace::from(project.urn()), peer.peer_id()),
        dst: GenericRef::heads(
            Flat,
            ext::RefLike::try_from(format!("{}@{}", owner.subject().name, peer.peer_id())).unwrap(),
        ),
        force: Force::True,
    }
    .into_fetchspec();

    let master = reflike!("refs/heads/master");

    let oid = create_commit(&repo, master.clone())?;
    let mut remote = Remote::rad_remote(url, fetchspec);
    remote
        .push(
            peer.clone(),
            &repo,
            LocalPushspec::Matching {
                pattern: refspec_pattern!("refs/heads/*"),
                force: Force::True,
            },
        )?
        .for_each(drop);

    peer.protocol()
        .announce(Gossip {
            origin: None,
            urn: project.urn().with_path(master),
            rev: Some(Rev::Git(oid)),
        })
        .await;

    Ok(oid)
}

// Create working copy of project
#[tracing::instrument(skip(peer), err)]
fn create_working_copy<P, I>(
    repo_path: P,
    inc_path: P,
    peer: &PeerApi,
    project: &Project,
    tracked_users: I,
) -> Result<git2::Repository, anyhow::Error>
where
    P: AsRef<Path> + Debug,
    I: IntoIterator<Item = (User, PeerId)> + Debug,
{
    let repo = git2::Repository::init(repo_path)?;

    let inc = include::Include::from_tracked_users(
        inc_path,
        LocalUrl::from(project.urn()),
        tracked_users.into_iter().map(|(user, peer_id)| {
            (
                ext::RefLike::try_from(user.doc.payload.subject.name.as_str()).unwrap(),
                peer_id,
            )
        }),
    );
    let inc_path = inc.file_path();
    inc.save()?;

    // Add the include above to include.path of the repo config
    include::set_include_path(&repo, inc_path)?;

    // Fetch from the working copy and check we have the commit in the working copy
    for remote in repo.remotes()?.iter().filter_map(identity) {
        let mut remote = Remote::find(&repo, ext::RefLike::try_from(remote).unwrap())?
            .expect("should exist, because libgit told us about it");
        remote
            .fetch(peer.clone(), &repo, LocalFetchspec::Configured)?
            .for_each(drop);
    }

    Ok(repo)
}

// Wait for peer2 to receive the gossip announcement
#[tracing::instrument(skip(peer_events))]
async fn wait_for_event<S>(peer_events: S, remote: PeerId)
where
    S: Stream<Item = PeerEvent> + std::marker::Unpin,
{
    tokio::time::timeout(
        Duration::from_secs(5),
        peer_events
            .filter(|event| match event {
                PeerEvent::GossipFetch(FetchInfo { provider, .. }) => {
                    future::ready(*provider == remote)
                },
            })
            .map(|_| ())
            .next(),
    )
    .await
    .unwrap();
}
