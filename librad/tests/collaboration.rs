// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#![feature(async_closure)]

use std::convert::TryFrom as _;

use librad::git::{
    identities,
    replication,
    types::{Namespace, Reference},
    Urn,
};

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
                let remote_peer = peer1.peer_id();
                let urn = project.urn();
                let addrs = peer1.listen_addrs().collect::<Vec<_>>();
                let limit = peer2.fetch_limit();
                move |storage| -> Result<(), anyhow::Error> {
                    replication::replicate(&storage, None, urn, remote_peer, addrs, limit)?;
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
                    identities::project::verify(storage, &urn)?;
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
                let limit = peer2.fetch_limit();
                let rad =
                    Urn::try_from(Reference::rad_id(Namespace::from(&urn)).with_remote(peer_id))
                        .unwrap();
                move |storage| -> Result<Option<identities::VerifiedProject>, anyhow::Error> {
                    replication::replicate(&storage, None, urn.clone(), peer_id, addrs, limit)?;
                    let project = identities::project::get(&storage, &rad)?.unwrap();
                    identities::project::update(
                        &storage,
                        &urn,
                        None,
                        None,
                        project.delegations().clone(),
                    )?;
                    identities::project::merge(&storage, &urn, peer_id)?;
                    Ok(identities::project::verify(storage, &urn)?)
                }
            })
            .await
            .unwrap()
            .unwrap();

        assert!(verified.is_some());
        {
            let peer_id = peer2.peer_id();
            assert_eq!(
                Some(peer_id.as_public_key()),
                verified.unwrap().delegations().iter().direct().next()
            );
        }
    })
    .await;
}
