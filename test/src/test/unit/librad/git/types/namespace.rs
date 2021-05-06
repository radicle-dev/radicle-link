// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::{
    git::types::namespace::Namespace,
    git_ext as ext,
    identities::urn::{test::FakeId, Urn},
    reflike,
};

#[test]
fn is_reflike() {
    let ns = Namespace::from(Urn::new(ext::Oid::from(git2::Oid::zero())));
    assert_eq!(
        "hnrkyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy",
        ext::RefLike::from(ns).as_str()
    )
}

#[test]
fn fake_is_reflike() {
    let ns = Namespace::from(Urn::new(FakeId(42)));
    assert_eq!("hyyryyyyyyyyyyybk", ext::RefLike::from(ns).as_str())
}

#[test]
fn strips_path_from_urn() {
    let ns = Namespace::from(Urn {
        id: FakeId(42),
        path: Some(reflike!("lolek/bolek")),
    });
    assert_eq!("hyyryyyyyyyyyyybk", ext::RefLike::from(ns).as_str())
}

#[test]
fn display_is_reflike_to_str() {
    let ns = Namespace::from(Urn::new(FakeId(69)));
    assert_eq!(&ns.to_string(), ext::RefLike::from(ns).as_str())
}

#[test]
fn reflike_from_ref_from_owned() {
    let ns = Namespace::from(Urn::new(FakeId(666)));
    assert_eq!(ext::RefLike::from(&ns), ext::RefLike::from(ns))
}
