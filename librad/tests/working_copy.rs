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

use std::{convert::TryFrom, marker::PhantomData, time::Duration};

use assert_matches::assert_matches;
use futures::{
    future,
    stream::{Stream, StreamExt},
};
use tempfile::tempdir;

use librad::{
    git::{
        include,
        local::{transport, url::LocalUrl},
        types::{remote::Remote, FlatRef, Force, NamespacedRef},
    },
    git_ext as ext,
    meta::{entity::Signatory, project::ProjectInfo},
    net::peer::{FetchInfo, Gossip, PeerEvent, Rev},
    peer::PeerId,
    signer::SomeSigner,
    uri::{self, RadUrn},
};

use librad_test::{
    git::initial_commit,
    logging,
    rad::{
        entity::{Alice, Radicle},
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

        let mut alice = Alice::new(peer1_key.public());
        let mut radicle = Radicle::new(&alice);
        {
            let resolves_to_alice = alice.clone();
            alice
                .sign(&peer1_key, &Signatory::OwnedKey, &resolves_to_alice)
                .unwrap();
            radicle
                .sign(
                    &peer1_key,
                    &Signatory::User(alice.urn()),
                    &resolves_to_alice,
                )
                .unwrap();
        }

        let radicle_urn = radicle.urn();

        {
            let alice = alice.clone();
            let radicle = radicle.clone();
            peer1
                .with_storage(move |storage| {
                    storage.create_repo(&alice).unwrap();
                    storage.create_repo(&radicle).unwrap();
                })
                .await
                .unwrap();
        }

        let tracked_users = {
            let url = radicle_urn.clone().into_rad_url(peer1.peer_id());
            peer2
                .with_storage(move |storage| {
                    storage.clone_repo::<ProjectInfo, _>(url, None).unwrap();
                    storage
                        .tracked(&radicle_urn)
                        .unwrap()
                        .map(|peer| {
                            storage
                                .get_rad_self_of(&radicle_urn, Some(peer))
                                .map(|user| (user, peer))
                        })
                        .collect::<Result<Vec<_>, _>>()
                        .unwrap()
                })
                .await
                .unwrap()
        };

        let tmp = tempdir().unwrap();

        // Perform commit and push to working copy on peer1
        let repo = git2::Repository::init(tmp.path().join("peer1")).unwrap();
        let url = LocalUrl::from_urn(radicle.urn(), peer1.peer_id());

        let heads = NamespacedRef::heads(radicle.urn().id, Some(peer1.peer_id()));
        let remotes = FlatRef::heads(
            PhantomData,
            ext::RefLike::try_from(format!("{}@{}", alice.name(), peer1.peer_id())).unwrap(),
        );

        let remote = Remote::rad_remote(url, Some(remotes.refspec(heads, Force::True).boxed()));

        let mut remote_callbacks = git2::RemoteCallbacks::new();
        remote_callbacks.push_update_reference(|refname, maybe_error| match maybe_error {
            None => {
                let rev = repo.find_reference(refname)?.target().unwrap();

                futures::executor::block_on(peer1.protocol().announce(Gossip {
                    origin: Some(peer1.peer_id()),
                    urn: RadUrn {
                        path: uri::Path::parse(refname).unwrap(),
                        ..radicle.urn()
                    },
                    rev: Some(Rev::Git(rev)),
                }));

                Ok(())
            },

            Some(err) => Err(git2::Error::from_str(&format!(
                "Remote rejected {}: {}",
                refname, err
            ))),
        });

        // Push a change and wait for peer2 to receive it in their monorepo
        let commit_id =
            initial_commit(&repo, remote, "refs/heads/master", Some(remote_callbacks)).unwrap();
        assert!(local_transport_results_peer1
            .wait(Duration::from_secs(3))
            .is_some());
        wait_for_event(peer2_events, peer1.peer_id()).await;

        // Create working copy of project
        let repo = git2::Repository::init(tmp.path().join("peer2")).unwrap();

        // Create the include file
        let url = LocalUrl {
            repo: radicle.urn().id,
            local_peer_id: peer2.peer_id(),
        };
        let inc = include::Include::from_tracked_users(tmp.path(), url, tracked_users.into_iter());
        let inc_path = inc.file_path();
        inc.save().unwrap();

        // Add the include above to include.path of the repo config
        include::set_include_path(&repo, inc_path).unwrap();

        // Fetch from the working copy and check we have the commit in the working copy
        for remote in repo.remotes().unwrap().iter() {
            let mut remote = repo.find_remote(remote.unwrap()).unwrap();
            remote.connect(git2::Direction::Fetch).unwrap();
            let remote_list = remote
                .list()
                .unwrap()
                .iter()
                .map(|head| head.name().to_string())
                .collect::<Vec<_>>();
            for name in remote_list {
                remote.fetch(&[&name], None, None).unwrap();
            }
        }
        for res in local_transport_results_peer2
            .wait(Duration::from_secs(5))
            .unwrap()
        {
            assert_matches!(res, Ok(_), "fetch error");
        }

        assert!(repo.find_commit(commit_id).is_ok());
    })
    .await;
}

// Wait for peer2 to receive the gossip announcement
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
