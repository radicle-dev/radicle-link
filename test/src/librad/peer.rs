// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use proptest::{collection, prelude::*};

use librad::PeerId;

use crate::librad::keys::gen_secret_key;

pub fn gen_peer_id() -> impl Strategy<Value = PeerId> {
    gen_secret_key().prop_map(PeerId::from)
}

pub fn gen_peers() -> impl Strategy<Value = (PeerId, Vec<PeerId>)> {
    gen_peer_id().prop_flat_map(move |local| {
        collection::vec(gen_peer_id(), 1..20).prop_map(move |remotes| {
            (
                local,
                remotes
                    .into_iter()
                    .filter(|remote| *remote != local)
                    .collect(),
            )
        })
    })
}
