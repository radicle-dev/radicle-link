// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::BTreeSet, iter};

use git_ref_format::RefString;
use git_ref_format_test as git_ref;
use librad::net::protocol::{
    membership::PartialView,
    request_pull,
    PartialPeerInfo,
    PeerAdvertisement,
};
use link_crypto::PeerId;
use link_crypto_test::gen::gen_peer_id;
use link_identities_test::gen::urn::gen_oid;
use proptest::{collection, prelude::*};

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

pub fn gen_request_pull_success() -> impl Strategy<Value = request_pull::Success> {
    (
        collection::vec(
            (git_ref::gen::valid(), gen_oid(git2::ObjectType::Commit)),
            1..3,
        ),
        collection::vec(git_ref::gen::valid(), 1..2),
    )
        .prop_map(move |(refs, pruned)| {
            let refs = refs
                .into_iter()
                .map(move |(n, t)| request_pull::Ref {
                    name: RefString::try_from(n).unwrap(),
                    oid: t,
                })
                .collect::<Vec<_>>();
            let pruned = pruned
                .into_iter()
                .map(|n| RefString::try_from(n).unwrap())
                .collect::<Vec<_>>();
            request_pull::Success { refs, pruned }
        })
}
