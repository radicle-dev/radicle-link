// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryFrom as _;

use librad::git::{
    identities,
    types::{Namespace, Reference},
    Urn,
};

use librad_test::{
    logging,
    rad::{identities::TestProject, testnet},
};

const NUM_PEERS: usize = 2;

#[tokio::test(core_threads = 2)]
async fn can_add_maintainer() {
    logging::init();

    let peers = testnet::setup(NUM_PEERS).await.unwrap();
    testnet::run_on_testnet(peers, NUM_PEERS, |mut apis| async move {
        let peer1 = apis.pop().unwrap();
        let peer2 = apis.pop().unwrap();

        let proj = {
            let proj = peer1
                .using_storage(move |storage| TestProject::create(&storage))
                .await
                .unwrap()
                .unwrap();
            proj.pull(&peer1, &peer2).await.ok().unwrap();
            proj
        };

        peer1
            .using_storage({
                let urn = proj.project.urn();
                let owner = proj.owner.clone();
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

        proj.pull(&peer1, &peer2).await.ok().unwrap();
        let verified = peer2
            .using_storage({
                let urn = proj.project.urn();
                let peer_id = peer1.peer_id();
                let rad =
                    Urn::try_from(Reference::rad_id(Namespace::from(&urn)).with_remote(peer_id))
                        .unwrap();
                move |storage| -> Result<Option<identities::VerifiedProject>, anyhow::Error> {
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
