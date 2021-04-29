// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::ops::Index as _;

use librad::{
    self,
    git::{self, refs::Refs, tracking},
    git_ext as ext,
    reflike,
};
use librad_test::{
    logging,
    rad::{identities::TestProject, testnet},
};

fn config() -> testnet::Config {
    testnet::Config {
        num_peers: nonzero!(3usize),
        min_connected: 3,
        bootstrap: testnet::Bootstrap::from_env(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sigrefs_dont_get_overwritten() {
    logging::init();

    let net = testnet::run(config()).await.unwrap();
    let peer1 = net.peers().index(0);
    let peer2 = net.peers().index(1);
    let peer3 = net.peers().index(2);

    let proj = peer1
        .using_storage(move |storage| TestProject::create(&storage))
        .await
        .unwrap()
        .unwrap();
    proj.pull(peer1, peer2).await.ok().unwrap();
    proj.pull(peer2, peer3).await.ok().unwrap();

    let commit_urn = proj.project.urn().with_path(reflike!("refs/heads/hi"));
    peer1
        .using_storage({
            let urn = proj.project.urn();
            let peer = peer2.peer_id();
            move |storage| -> Result<bool, anyhow::Error> {
                git::util::quick_commit(
                    &storage,
                    &commit_urn,
                    vec![("README.md", ext::tree::blob(b"Hello, Radicle"))]
                        .into_iter()
                        .collect(),
                    "initial commit",
                )?;
                Ok(tracking::track(storage, &urn, peer)?)
            }
        })
        .await
        .unwrap()
        .unwrap();

    proj.pull(peer1, peer2).await.unwrap();
    proj.pull(peer2, peer3).await.unwrap();

    let sigrefs = peer2
        .using_storage({
            let peer = peer1.peer_id();
            let urn = proj.project.urn();
            move |storage| Refs::load(storage, &urn, peer)
        })
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let expected = peer1
        .using_storage({
            let urn = proj.project.urn();
            move |storage| Refs::load(storage, &urn, None)
        })
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    assert_eq!(expected.heads, sigrefs.heads)
}
