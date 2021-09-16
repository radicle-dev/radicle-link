// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryFrom as _;

use proptest::{self as prop, prelude::*};

use librad::{
    git_ext::{self as ext, Oid},
    identities::urn::Urn,
};

pub fn gen_oid(kind: git2::ObjectType) -> impl Strategy<Value = Oid> {
    any::<Vec<u8>>()
        .prop_map(move |bytes| git2::Oid::hash_object(kind, &bytes).map(Oid::from).unwrap())
}

pub fn gen_urn() -> impl Strategy<Value = Urn<Oid>> {
    (
        gen_oid(git2::ObjectType::Tree),
        prop::option::of(prop::collection::vec("[a-z0-9]+", 1..3)),
    )
        .prop_map(|(id, path)| {
            let path = path.map(|elems| {
                ext::RefLike::try_from(elems.join("/")).unwrap_or_else(|e| {
                    panic!(
                        "Unexpected error generating a RefLike from `{}`: {}",
                        elems.join("/"),
                        e
                    )
                })
            });
            Urn { id, path }
        })
}
