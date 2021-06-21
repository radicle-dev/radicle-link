// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom as _, ops::Index as _};

use librad::git::{
    identities,
    storage::ReadOnlyStorage as _,
    types::{Namespace, Reference},
    Urn,
};

use crate::{
    logging,
    rad::{
        identities::{TestPerson, TestProject},
        testnet,
    },
};

fn config() -> testnet::Config {
    testnet::Config {
        num_peers: nonzero!(2usize),
        min_connected: 2,
        bootstrap: testnet::Bootstrap::from_env(),
    }
}

#[test]
fn can_replicate_with_updated_delegate() {
    logging::init();

    let net = testnet::run(config()).unwrap();
    net.enter(async {
        let peer1 = net.peers().index(0);
        let peer2 = net.peers().index(1);

        let person = {
            let person = peer1
                .using_storage(move |storage| TestPerson::create(storage))
                .await
                .unwrap()
                .unwrap();
            person.pull(peer1, peer2).await.ok().unwrap();
            person
        };

        let has = peer2
            .using_storage({
                let urn = person.owner.urn();
                move |storage| storage.has_urn(&urn)
            })
            .await
            .unwrap()
            .unwrap();
        assert!(has);

        let person = {
            let person = peer1
                .using_storage(move |storage| person.update(storage))
                .await
                .unwrap()
                .unwrap();
            person
        };

        let proj = {
            let proj = peer1
                .using_storage(move |storage| TestProject::from_test_person(storage, person))
                .await
                .unwrap()
                .unwrap();
            proj.pull(peer1, peer2).await.unwrap();
            proj
        };

        let has = peer2
            .using_storage({
                let urn = proj.project.urn();
                move |storage| storage.has_urn(&urn)
            })
            .await
            .unwrap()
            .unwrap();
        assert!(has);

        let parity = peer2
            .using_storage({
                let urn = proj.owner.urn();
                let delegate = Reference::rad_delegate(Namespace::from(proj.project.urn()), &urn);
                move |storage| -> anyhow::Result<bool> {
                    let pers = identities::person::verify(&storage, &urn)?.unwrap();
                    let del =
                        identities::person::verify(&storage, &Urn::try_from(delegate).unwrap())?
                            .unwrap();
                    Ok(pers.content_id == del.content_id)
                }
            })
            .await
            .unwrap()
            .unwrap();
        assert!(parity)
    })
}
