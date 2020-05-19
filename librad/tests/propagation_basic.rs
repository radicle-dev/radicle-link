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

use std::{future::Future, time::Duration};

use futures::future::Either;
use futures_timer::Delay;

use librad::{
    internal::sync::Monitor,
    meta::{entity::Signatory, project::ProjectInfo},
    net::peer::{BoundPeer, Gossip, Rev},
    peer::PeerId,
    uri::{self, RadUrn},
};

mod fixtures;
mod logging;
mod testnet;

use fixtures::{Alice, Radicle};

#[tokio::test]
async fn can_clone() {
    logging::init();

    let peers = testnet::setup(2).unwrap();
    let bound = testnet::bind(&peers).await.unwrap();

    let urn = {
        let peer1 = peers[0].peer.clone();
        let peer2 = peers[1].peer.clone();

        let alice = Alice::new(peer1.public_key());
        let mut radicle = Radicle::new(&alice);
        let urn = radicle.urn();

        run_on_testnet(bound, async move {
            radicle
                .sign(peer1.key(), &Signatory::User(alice.urn()), &alice)
                .await
                .unwrap();

            tokio::task::spawn_blocking(move || {
                let git1 = peer1.git().clone();
                let git2 = peer2.git().clone();
                git1.create_repo(&radicle).unwrap();
                git2.clone_repo::<ProjectInfo>(
                    radicle.urn().into_rad_url(PeerId::from(peer1.public_key())),
                )
                .unwrap();
            })
            .await
            .unwrap()
        })
        .await;

        urn
    };

    let git1 = peers[1].peer.git().clone();
    let _ = git1.open_repo(urn).unwrap();
}

#[tokio::test]
async fn fetches_on_gossip_notify() {
    logging::init();

    let peers = testnet::setup(2).unwrap();
    let bound = testnet::bind(&peers).await.unwrap();

    let (urn, commit_id) = {
        let peer1 = peers[0].peer.clone();
        let peer2 = peers[1].peer.clone();

        let alice = Alice::new(peer1.public_key());
        let mut radicle = Radicle::new(&alice);

        let peer1_handle = bound[0].handle();

        run_on_testnet(bound, async move {
            radicle
                .sign(peer1.key(), &Signatory::User(alice.urn()), &alice)
                .await
                .unwrap();

            let peer1_id = PeerId::from(peer1.public_key());

            let (urn, commit_id) = tokio::task::spawn_blocking(move || {
                // Create a repo on peer1 and have peer2 clone it
                let peer1_git = peer1.git();
                let mut peer1_repo = peer1_git.clone().create_repo(&radicle).unwrap();
                let peer1_project_urn = peer1_repo.urn();

                let peer2_git = peer2.git();
                peer2_git
                    .clone()
                    .clone_repo::<ProjectInfo>(
                        peer1_project_urn
                            .clone()
                            .into_rad_url(PeerId::from(peer1.public_key())),
                    )
                    .unwrap();

                // Create a commit in peer1's repo and gossip the head rev
                // FIXME: should operate on a working copy + push
                {
                    let repo = peer1_repo.locked();

                    let empty_tree = {
                        let mut index = repo.index().unwrap();
                        let oid = index.write_tree().unwrap();
                        repo.find_tree(oid).unwrap()
                    };
                    let commit_id = repo
                        .commit("master", "Initial commit", &empty_tree, &[])
                        .unwrap();

                    (
                        RadUrn {
                            path: uri::Path::parse("refs/heads/master").unwrap(),
                            ..peer1_project_urn
                        },
                        commit_id,
                    )
                }
            })
            .await
            .unwrap();

            peer1_handle
                .announce(Gossip {
                    urn: urn.clone(),
                    rev: Some(Rev::Git(commit_id)),
                    origin: peer1_id,
                })
                .await;

            // Wait a moment for peer2 to react
            // FIXME: add an event chan to peer, so we can tell when it's done
            // (or time out)
            let _ = tokio::task::spawn(Delay::new(Duration::from_secs(5))).await;

            (urn, commit_id)
        })
        .await
    };

    // Check peer2 fetched the gossiped update
    let peer1 = &peers[0].peer;
    let peer2 = &peers[1].peer;
    assert!(peer2.git_has(peer2.urn_context(urn, peer1.peer_id()), commit_id))
}

async fn run_on_testnet<F, A>(bound: Vec<BoundPeer<'_>>, f: F) -> A
where
    F: Future<Output = A>,
{
    let handles = bound.iter().map(|b| b.handle()).collect();
    let connected = testnet::wait_connected(handles, bound.len());

    let res = futures::future::select(
        Box::pin(testnet::run(bound, Monitor::new())),
        Box::pin(async {
            connected.await;
            f.await
        }),
    )
    .await;

    match res {
        Either::Left(_) => unreachable!(),
        Either::Right((output, _)) => output,
    }
}
