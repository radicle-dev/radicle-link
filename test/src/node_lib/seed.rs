// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

pub mod store;

use proptest::prelude::*;

use node_lib::Seed;

use crate::librad::peer::gen_peer_id;

pub fn gen_seed() -> impl Strategy<Value = Seed<String>> {
    gen_peer_id().prop_map(move |peer| Seed {
        peer,
        addrs: "localhost".to_string(),
        label: None,
    })
}
