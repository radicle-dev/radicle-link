// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::collections::BTreeSet;

use librad::{
    git::{
        tracking::{migration, tracked_peers, v1},
        Storage,
    },
    paths::Paths,
    SecretKey,
};
use link_crypto_test::gen::gen_peer_id;
use link_identities_test::gen::urn::gen_urn;
use proptest::prelude::*;

proptest! {
#[test]
    fn migration(
        urns in prop::collection::vec(gen_urn(), 1..5),
        peers in prop::collection::btree_set(gen_peer_id(), 1..5)
    ) {
        let tmp = tempfile::tempdir().unwrap();
        {
            let paths = Paths::from_root(&tmp).unwrap();
            let storage = Storage::open(&paths, SecretKey::new()).unwrap();

            for urn in &urns {
                for peer in &peers {
                    v1::track(&storage, urn, *peer).unwrap();
                }
            }

            migration::migrate(&storage, urns.clone()).unwrap();
            for urn in urns {
                assert_eq!(
                    peers,
                    tracked_peers(&storage, Some(&urn))
                        .unwrap()
                        .collect::<Result<BTreeSet<_>, _>>()
                        .unwrap()
                );

                assert!(v1::tracked(&storage, &urn)
                        .unwrap()
                        .next().is_none());
            }
        }
    }
}
