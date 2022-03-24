// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use link_crypto_test::gen::gen_peer_id;
use lnk_clib::seed::Seed;
use proptest::prelude::Strategy;

pub fn seed() -> impl Strategy<Value = Seed<String>> {
    gen_peer_id().prop_map(move |peer| Seed {
        peer,
        addrs: "localhost".to_string(),
        label: None,
    })
}
