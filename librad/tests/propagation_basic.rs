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
    meta::{project::ProjectInfo, Project, User},
    net::peer::{BoundPeer, Gossip, Rev},
    peer::PeerId,
};

mod logging;
mod testnet;

#[tokio::test]
async fn can_clone() {
    logging::init();

    let peers = testnet::setup(2).unwrap();
    let bound = testnet::bind(&peers).await.unwrap();

    let urn = {
        let peer1 = peers[0].peer.clone();
        let peer2 = peers[1].peer.clone();

        let peer1_user = User::new("alice".to_owned(), peer1.public_key()).unwrap();
        let peer1_project = Project::new("radicle".to_owned(), &peer1_user.urn()).unwrap();
        let urn = peer1_project.urn();

        run_on_testnet(bound, async move {
            tokio::task::spawn_blocking(move || {
                let git1 = peer1.git().clone();
                let git2 = peer2.git().clone();

                println!("create repo");
                git1.create_repo(&peer1_project).unwrap();
                println!("clone");
                git2.clone_repo::<ProjectInfo>(
                    peer1_project
                        .urn()
                        .into_rad_url(PeerId::from(peer1.public_key())),
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

        let peer1_user = User::new("alice".to_owned(), peer1.public_key()).unwrap();
        let peer1_project = Project::new("radicle".to_owned(), &peer1_user.urn()).unwrap();
        let peer1_project_urn = peer1_project.urn();

        let peer1_handle = bound[0].handle();

        let commit_id = run_on_testnet(bound, async move {
            let (commit_id, urn) = tokio::task::spawn_blocking(move || {
                // Create a repo on peer1 and have peer2 clone it
                let peer1_git = peer1.git();
                let peer1_repo = peer1_git.clone().create_repo(&peer1_project).unwrap();
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
                let commit_id = peer1_git
                    .clone()
                    .create_empty_commit(peer1_project_urn.clone())
                    .unwrap();

                (commit_id, peer1_project_urn.clone())
            })
            .await
            .unwrap();

            peer1_handle
                .announce(Gossip {
                    urn,
                    rev: Rev::Git(commit_id),
                })
                .await;

            // Wait a moment for peer2 to react
            let _ = tokio::task::spawn(Delay::new(Duration::from_secs(2))).await;

            commit_id
        })
        .await;

        (peer1_project_urn, commit_id)
    };

    // Check peer2 fetched the gossiped update
    assert!(peers[1].peer.git_has(urn, commit_id))
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
