// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use proptest::prelude::*;

use link_crypto::PeerId;
use link_crypto_test::gen::gen_peer_id;
use link_hooks::Track;
use link_identities_test::gen::urn::{gen_oid_with_zero, gen_urn};
use radicle_git_ext as ext;

pub fn gen_track() -> impl Strategy<Value = Track<ext::Oid>> {
    default_or_peer().prop_flat_map(move |peer| {
        gen_oid_with_zero(git2::ObjectType::Commit).prop_flat_map(move |old| {
            gen_oid_with_zero(git2::ObjectType::Commit).prop_flat_map(move |new| {
                gen_urn().prop_map(move |urn| Track {
                    urn,
                    peer,
                    old,
                    new,
                })
            })
        })
    })
}

fn default_or_peer() -> impl Strategy<Value = Option<PeerId>> {
    prop_oneof![Just(None), gen_peer_id().prop_map(Some)]
}
