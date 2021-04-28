// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    convert::{TryFrom, TryInto},
    ops::Index as _,
};

use librad::{
    self,
    git::{
        local::url::LocalUrl,
        refs::Refs,
        storage::Storage,
        tracking,
        types::{remote, Flat, Force, GenericRef, Namespace, Reference, Refspec, Remote},
        Urn,
    },
    git_ext as ext,
    peer::PeerId,
    reflike,
    refspec_pattern,
};
use librad_test::{
    git::create_commit,
    logging,
    rad::{identities::TestProject, testnet},
};
use tokio::task::spawn_blocking;

fn config() -> testnet::Config {
    testnet::Config {
        num_peers: nonzero!(3usize),
        min_connected: 0,
        bootstrap: testnet::Bootstrap::None,
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
    let commit_id = spawn_blocking({
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
    .await
    .unwrap();

    let expected_urn = proj.project.urn().with_path(
        reflike!("refs/remotes")
            .join(peer1.peer_id())
            .join(reflike!("heads"))
            .join(&default_branch),
    );

    println!("PULLING PEER 1");
    proj.pull(peer1, peer2).await.ok().unwrap();
    println!("PULLING PEER 2");
    proj.pull(peer2, peer3).await.ok().unwrap();

    println!("GETTING SIGREFS 2");
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
    println!("GETTING SIGREFS 1");
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
