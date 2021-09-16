// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::BTreeSet, iter};

use proptest::prelude::*;

use librad::{
    net::protocol::{membership::PartialView, PartialPeerInfo, PeerAdvertisement},
    PeerId,
};

use crate::librad::peer::gen_peer_id;

pub fn gen_partial_view() -> impl Strategy<Value = PartialView<rand::rngs::ThreadRng, ()>> {
    gen_peer_id().prop_flat_map(|local_id| {
        any::<(usize, usize)>().prop_map(move |(max_active, max_passive)| {
            PartialView::new(local_id, rand::thread_rng(), max_active, max_passive)
        })
    })
}

pub fn blank_peer_info<A: Ord + Clone>(peer_id: PeerId) -> PartialPeerInfo<A> {
    PartialPeerInfo {
        peer_id,
        advertised_info: Some(PeerAdvertisement {
            listen_addrs: iter::empty().into(),
            capabilities: BTreeSet::new(),
        }),
        seen_addrs: iter::empty().into(),
    }
}
