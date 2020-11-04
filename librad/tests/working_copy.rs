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

use std::{convert::TryFrom, fmt::Debug, marker::PhantomData, path::Path, time::Duration};

use assert_matches::assert_matches;
use futures::{
    future,
    stream::{Stream, StreamExt},
};
use tempfile::tempdir;

use librad::{
    git::{
        identities::{self, Project, User},
        include,
        local::{transport, url::LocalUrl},
        replication,
        tracking,
        types::{namespace::Namespace, remote::Remote, FlatRef, Force, NamespacedRef},
        Urn,
    },
    git_ext as ext,
    net::peer::{FetchInfo, Gossip, PeerApi, PeerEvent, Rev},
    peer::PeerId,
    signer::{Signer, SomeSigner},
};

use librad_test::{
    git::initial_commit,
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
        let (peer1, peer1_key) = apis.pop().unwrap();
        let (peer2, peer2_key) = apis.pop().unwrap();
        let peer2_events = peer2.subscribe().await;

        librad::git::local::transport::register();
        let local_transport_results_peer1 =
            transport::LocalTransportFactory::configure(transport::Settings {
                paths: peer1.paths().clone(),
                signer: SomeSigner { signer: peer1_key }.into(),
            });
        let local_transport_results_peer2 =
            transport::LocalTransportFactory::configure(transport::Settings {
                paths: peer2.paths().clone(),
                signer: SomeSigner { signer: peer2_key }.into(),
            });

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
                    eprintln!("listing tracked for {}", urn);

                    tracking::tracked(&store, &urn)
                        .unwrap()
                        .map(|peer| {
                            let self_ref = NamespacedRef::rad_self(Namespace::from(&urn), peer);
                            let user = identities::user::get(&store, &Urn::from(self_ref))
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
            let commit_id =
                commit_and_push(tmp.path().join("peer1"), &peer1, &owner, &project).unwrap();

            for res in local_transport_results_peer1
                .wait(Duration::from_secs(5))
                .expect("there should have been push activity")
            {
                assert_matches!(res, Ok(_), "push error");
            }

            wait_for_event(peer2_events, peer1.peer_id()).await;

            let peer2_repo = create_working_copy(
                tmp.path().join("peer2"),
                tmp.path().to_path_buf(),
                &peer2,
                &project,
                tracked_users,
            )
            .unwrap();

            for res in local_transport_results_peer2
                .wait(Duration::from_secs(5))
                .expect("there should have been fetch activity")
            {
                assert_matches!(res, Ok(_), "fetch error");
            }

            assert!(peer2_repo.find_commit(commit_id).is_ok());
        }
    })
    .await;
}

// Perform commit and push to working copy on peer1
#[tracing::instrument(skip(peer), err)]
fn commit_and_push<P, S>(
    repo_path: P,
    peer: &PeerApi<S>,
    owner: &User,
    project: &Project,
) -> Result<git2::Oid, anyhow::Error>
where
    P: AsRef<Path> + Debug,
    S: Signer + Clone,
{
    let repo = git2::Repository::init(repo_path)?;
    let url = LocalUrl::from_urn(project.urn(), peer.peer_id());

    let heads = NamespacedRef::heads(Namespace::from(project.urn()), peer.peer_id());
    let remotes = FlatRef::heads(
        PhantomData,
        ext::RefLike::try_from(format!(
            "{}@{}",
            owner.doc.payload.subject.name,
            peer.peer_id()
        ))
        .unwrap(),
    );

    let fetchspec = remotes.refspec(heads, Force::True);
    let remote = Remote::rad_remote(url, fetchspec.boxed());

    let mut updated_refs = Vec::new();
    let mut remote_callbacks = git2::RemoteCallbacks::new();
    remote_callbacks.push_update_reference(|refname, maybe_error| match maybe_error {
        None => {
            let rev = repo.find_reference(refname)?.target().unwrap();
            let refname = ext::RefLike::try_from(refname).unwrap();
            updated_refs.push((refname, rev));

            Ok(())
        },

        Some(err) => Err(git2::Error::from_str(&format!(
            "Remote rejected {}: {}",
            refname, err
        ))),
    });

    let oid = initial_commit(&repo, remote, "refs/heads/master", Some(remote_callbacks))?;

    for (path, rev) in updated_refs {
        futures::executor::block_on(peer.protocol().announce(Gossip {
            origin: Some(peer.peer_id()),
            urn: project.urn().with_path(path),
            rev: Some(Rev::Git(rev)),
        }))
    }

    Ok(oid)
}

// Create working copy of project
#[tracing::instrument(skip(peer), err)]
fn create_working_copy<P, S, I>(
    repo_path: P,
    inc_path: P,
    peer: &PeerApi<S>,
    project: &Project,
    tracked_users: I,
) -> Result<git2::Repository, anyhow::Error>
where
    P: AsRef<Path> + Debug,
    S: Signer + Clone,
    I: IntoIterator<Item = (User, PeerId)> + Debug,
{
    let repo = git2::Repository::init(repo_path)?;

    let inc = include::Include::from_tracked_users(
        inc_path,
        LocalUrl {
            urn: project.urn(),
            local_peer_id: peer.peer_id(),
        },
        tracked_users,
    );
    let inc_path = inc.file_path();
    inc.save()?;

    // Add the include above to include.path of the repo config
    include::set_include_path(&repo, inc_path)?;

    // Fetch from the working copy and check we have the commit in the working copy
    for remote in repo.remotes()?.iter() {
        let mut remote = repo.find_remote(remote.unwrap())?;
        remote.connect(git2::Direction::Fetch)?;
        let remote_list = remote
            .list()
            .unwrap()
            .iter()
            .map(|head| head.name().to_string())
            .collect::<Vec<_>>();
        for name in remote_list {
            tracing::debug!("fetching {}", name);
            remote.fetch(&[&name], None, None)?;
        }
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
