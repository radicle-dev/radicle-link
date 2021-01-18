// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#![feature(async_closure)]

use librad::git::{identities, replication};

use librad_test::{
    logging,
    rad::{
        identities::{create_test_project, TestProject},
        testnet,
    },
};

#[tokio::test(core_threads = 2)]
async fn can_add_maintainer() {
    logging::init();

    const NUM_PEERS: usize = 3;

    let peers = testnet::setup(NUM_PEERS).await.unwrap();
    testnet::run_on_testnet(peers, NUM_PEERS, async move |mut apis| {
        let (peer1, _) = apis.pop().unwrap();
        let (peer2, _) = apis.pop().unwrap();

        peer1
            .with_storage(move |storage| {
                println!("PATH: {}", storage.path().display());
            })
            .await
            .unwrap();
        peer2
            .with_storage(move |storage| {
                println!("PATH: {}", storage.path().display());
            })
            .await
            .unwrap();

        let TestProject { project, owner } = peer1
            .with_storage(move |storage| create_test_project(&storage))
            .await
            .unwrap()
            .unwrap();

        peer2
            .with_storage({
                let remote_peer = peer1.peer_id();
                let urn = project.urn();
                let addrs = peer1.listen_addrs().collect::<Vec<_>>();
                move |storage| -> Result<(), anyhow::Error> {
                    replication::replicate(&storage, None, urn, remote_peer, addrs)?;
                    Ok(())
                }
            })
            .await
            .unwrap()
            .unwrap();

        peer2
            .with_storage({
                let urn = project.urn();
                let peer_id = peer2.peer_id();
                let key = *peer_id.as_public_key();
                let owner = owner.clone();
                move |storage| -> Result<(), anyhow::Error> {
                    identities::project::update(
                        storage,
                        &urn,
                        None,
                        None,
                        librad::identities::delegation::Indirect::try_from_iter(
                            vec![either::Either::Left(key), either::Either::Right(owner)]
                                .into_iter(),
                        )
                        .unwrap(),
                    )?;
                    identities::project::verify(storage, &urn)?.expect("Should be there like");
                    Ok(())
                }
            })
            .await
            .unwrap()
            .unwrap();

        peer1
            .with_storage({
                let urn = project.urn();
                let peer_id = peer2.peer_id();
                let addrs = peer2.listen_addrs().collect::<Vec<_>>();
                move |storage| -> Result<(), anyhow::Error> {
                    librad::git::tracking::track(&storage, &urn, peer_id)?;
                    replication::replicate(&storage, None, urn.clone(), peer_id, addrs)?;
                    identities::project::merge(storage, &urn, peer_id)?;
                    identities::project::verify(storage, &urn)?.expect("Should be there like");
                    Ok(())
                }
            })
            .await
            .unwrap()
            .unwrap();

        let verified = peer2
            .with_storage({
                let urn = project.urn();
                let peer_id = peer1.peer_id();
                let addrs = peer2.listen_addrs().collect::<Vec<_>>();
                move |storage| -> Result<Option<identities::VerifiedProject>, anyhow::Error> {
                    replication::replicate(&storage, None, urn.clone(), peer_id, addrs)?;
                    identities::project::merge(storage, &urn, peer_id)?;
                    Ok(identities::project::verify(storage, &urn)?)
                }
            })
            .await
            .unwrap()
            .unwrap();
        // std::thread::sleep(std::time::Duration::from_secs(60));

        assert!(verified.is_some());
    })
    .await;
}

#[tokio::test(core_threads = 2)]
async fn adding_maintainers_commutes() {
    logging::init();

    const NUM_PEERS: usize = 3;

    let peers = testnet::setup(NUM_PEERS).await.unwrap();
    testnet::run_on_testnet(peers, NUM_PEERS, async move |mut apis| {
        let (peer1, _) = apis.pop().unwrap();
        let (peer2, _) = apis.pop().unwrap();

        peer1
            .with_storage(move |storage| {
                println!("PATH: {}", storage.path().display());
            })
            .await
            .unwrap();
        peer2
            .with_storage(move |storage| {
                println!("PATH: {}", storage.path().display());
            })
            .await
            .unwrap();

        let TestProject { project, owner } = peer1
            .with_storage(move |storage| create_test_project(&storage))
            .await
            .unwrap()
            .unwrap();

        peer2
            .with_storage({
                let remote_peer = peer1.peer_id();
                let urn = project.urn();
                let addrs = peer1.listen_addrs().collect::<Vec<_>>();
                move |storage| -> Result<(), anyhow::Error> {
                    replication::replicate(&storage, None, urn, remote_peer, addrs)?;
                    Ok(())
                }
            })
            .await
            .unwrap()
            .unwrap();

        peer2
            .with_storage({
                let urn = project.urn();
                let peer_id = peer2.peer_id();
                let key = *peer_id.as_public_key();
                let owner = owner.clone();
                move |storage| -> Result<(), anyhow::Error> {
                    identities::project::update(
                        storage,
                        &urn,
                        None,
                        None,
                        librad::identities::delegation::Indirect::try_from_iter(
                            vec![either::Either::Left(key), either::Either::Right(owner)]
                                .into_iter(),
                        )
                        .unwrap(),
                    )?;
                    identities::project::verify(storage, &urn)?.expect("project went missing");
                    Ok(())
                }
            })
            .await
            .unwrap()
            .unwrap();

        peer1
            .with_storage({
                let urn = project.urn();
                let peer_id = peer2.peer_id();
                let key = *peer_id.as_public_key();
                let owner = owner.clone();
                move |storage| -> Result<(), anyhow::Error> {
                    identities::project::update(
                        storage,
                        &urn,
                        None,
                        None,
                        librad::identities::delegation::Indirect::try_from_iter(
                            vec![either::Either::Left(key), either::Either::Right(owner)]
                                .into_iter(),
                        )
                        .unwrap(),
                    )?;
                    identities::project::verify(storage, &urn)?.expect("project went missing");
                    Ok(())
                }
            })
            .await
            .unwrap()
            .unwrap();

        let to_me = peer1
            .with_storage({
                let urn = project.urn();
                let peer_id = peer2.peer_id();
                let addrs = peer2.listen_addrs().collect::<Vec<_>>();
                move |storage| -> Result<identities::VerifiedProject, anyhow::Error> {
                    librad::git::tracking::track(&storage, &urn, peer_id)?;
                    replication::replicate(&storage, None, urn.clone(), peer_id, addrs)?;
                    identities::project::merge(storage, &urn, peer_id)?;
                    Ok(identities::project::verify(storage, &urn)?.expect("project went missing"))
                }
            })
            .await
            .unwrap()
            .unwrap();

        let to_you = peer2
            .with_storage({
                let urn = project.urn();
                let peer_id = peer1.peer_id();
                let addrs = peer2.listen_addrs().collect::<Vec<_>>();
                move |storage| -> Result<identities::VerifiedProject, anyhow::Error> {
                    replication::replicate(&storage, None, urn.clone(), peer_id, addrs)?;
                    identities::project::merge(storage, &urn, peer_id)?;
                    Ok(identities::project::verify(storage, &urn)?.expect("project went missing"))
                }
            })
            .await
            .unwrap()
            .unwrap();

        assert_eq!(to_me.revision, to_you.revision);
    })
    .await;
}