// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryFrom as _;

use librad::{git_ext as ext, identities::urn::Urn};

#[test]
fn is_reflike() {
    assert_eq!(
        "hnrkyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy",
        ext::RefLike::from(Urn::new(ext::Oid::from(git2::Oid::zero()))).as_str()
    )
}

#[test]
fn is_reflike_with_path() {
    assert_eq!(
        "hnrkyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy/refs/heads/lolek/bolek",
        ext::RefLike::from(Urn {
            id: ext::Oid::from(git2::Oid::zero()),
            path: Some(ext::RefLike::try_from("lolek/bolek").unwrap())
        })
        .as_str()
    )
}

#[test]
fn is_reflike_with_qualified_path() {
    assert_eq!(
        "hnrkyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy/refs/remotes/lolek/bolek",
        ext::RefLike::from(Urn {
            id: ext::Oid::from(git2::Oid::zero()),
            path: Some(ext::RefLike::try_from("refs/remotes/lolek/bolek").unwrap())
        })
        .as_str()
    )
}
