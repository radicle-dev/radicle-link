// Copyright © 2022 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::collections::BTreeSet;

use proptest::{collection, prelude::*};

use node_lib::seed::store::Store as _;

use crate::node_lib::seed::{gen_seed, store::kv_store};

proptest! {
    // NOTE: limiting the number of cases since we write to the tempfile on every run
    #![proptest_config(ProptestConfig {
        cases: 64, .. ProptestConfig::default()
    })]

    #[test]
    fn read(seeds in collection::vec(gen_seed(), 1..5)) {
        let store = kv_store(seeds.clone());
        let seeds = seeds.into_iter().collect::<BTreeSet<_>>();
        assert_eq!(
            store
                .scan()
                .unwrap()
                .collect::<Result<BTreeSet<_>, _>>()
                .unwrap(),
            seeds
        );
    }
}
