// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use crate::librad::peer::gen_peer_id;
use std::collections::BTreeMap;

use librad::{
    git::{
        refs::{Refs, Remotes},
        types::RefsCategory,
    },
    PeerId,
};
use proptest::{prelude::*, strategy::Strategy};
use radicle_git_ext::{reference, Oid, RefLike};

// Convenience macro for making a BTreeMap<String, BTreeMap<String, Oid>>
#[macro_export]
macro_rules! make_refs{
    ($($category:literal => {$($reference:literal => $oid:expr,)*},)*) => {
        {
            use std::collections::BTreeMap;
            let mut refs: BTreeMap<String, BTreeMap<String, radicle_git_ext::Oid>> = BTreeMap::new();
            $(
                #[allow(unused_variables)]
                {
                    let cat = refs.entry($category.to_string()).or_insert_with(BTreeMap::new);
                    $(
                        cat.insert($reference.to_string(), $oid.into());
                    )*
                }
            )*
            refs
        }
    }
}

use crate::librad::identities::urn::gen_oid;

fn gen_remotes() -> impl Strategy<Value = Remotes<PeerId>> {
    let leaf: Just<Remotes<PeerId>> = Just(Remotes::new());
    leaf.prop_recursive(3, 20, 10, |element| {
        proptest::collection::btree_map(gen_peer_id(), element, 3).prop_map(|bm| {
            let boxed: BTreeMap<PeerId, Box<Remotes<PeerId>>> =
                bm.into_iter().map(|(k, v)| (k, Box::new(v))).collect();
            boxed.into()
        })
    })
}

fn gen_ref_prefix() -> impl Strategy<Value = Option<String>> {
    proptest::option::of(proptest::string::string_regex("[a-zA-Z]{2,10}").unwrap())
}

prop_compose! {
    fn gen_onelevel()(
        ref_prefix in gen_ref_prefix(),
        names in proptest::collection::vec(proptest::string::string_regex("[a-z]{2,10}").unwrap(), 1..5)
        )-> reference::OneLevel {
        let joined: String = names.join("/");
        let string_reflike = match ref_prefix {
            Some(category) => format!("refs/{}/{}", category, joined),
            None => joined,
        };
        println!("string: {}", string_reflike);
        let reflike: RefLike = string_reflike.parse().unwrap();
        reference::OneLevel::from(reflike)
    }
}

fn gen_references() -> impl Strategy<Value = BTreeMap<reference::OneLevel, Oid>> {
    proptest::collection::btree_map(gen_onelevel(), gen_oid(git2::ObjectType::Commit), 0..10)
}

fn gen_unknown() -> impl Strategy<Value = BTreeMap<String, BTreeMap<String, Oid>>> {
    let submap =
        proptest::collection::btree_map(any::<String>(), gen_oid(git2::ObjectType::Commit), 0..5);
    proptest::collection::btree_map(proptest::arbitrary::any::<String>(), submap, 0..5)
}

prop_compose! {
    pub fn gen_refs()(
            heads in gen_references(),
            notes in gen_references(),
            cobs in proptest::option::of(gen_references()),
            tags in gen_references(),
            rad in gen_references(),
            unknown_categories in gen_unknown(),
            remotes in gen_remotes()
        ) -> Refs {
        let mut with_categories = vec![
            (RefsCategory::Heads, heads),
            (RefsCategory::Notes, notes),
            (RefsCategory::Tags, tags),
            (RefsCategory::Rad, rad),
        ];
        if let Some(cobs) = cobs {
            with_categories.push((RefsCategory::Cobs, cobs));
        }
        let mut all_categories = BTreeMap::new();
        for (category, refs) in with_categories {
            all_categories.insert(
                category.to_string(),
                refs.into_iter()
                    .map(|(r, oid)| (r.to_string(), oid))
                    .collect()
            );
        }
        all_categories.extend(unknown_categories);
        Refs {
            categorised_refs: all_categories,
            remotes,
        }
    }
}
