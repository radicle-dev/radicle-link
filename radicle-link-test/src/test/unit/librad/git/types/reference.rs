// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryFrom;

use librad::{
    git::{types::reference::Reference, Urn},
    git_ext as ext,
    identities,
    keys::SecretKey,
    peer::PeerId,
    reflike,
};

#[test]
fn pathless_urn_roundtrip() {
    let urn = Urn::new(git2::Oid::zero().into());
    let as_ref = Reference::try_from(&urn).unwrap();
    assert_eq!(
        urn.with_path(ext::RefLike::from(identities::urn::DEFAULT_PATH.clone())),
        Urn::try_from(as_ref).unwrap()
    )
}

#[test]
fn remotes_path_urn_roundtrip() {
    let peer_id = PeerId::from(SecretKey::new());
    let urn = Urn::new(git2::Oid::zero().into()).with_path(
        reflike!("refs/remotes")
            .join(peer_id)
            .join(reflike!("rad/id")),
    );
    let as_ref = Reference::try_from(&urn).unwrap();
    assert_eq!(urn, Urn::try_from(as_ref).unwrap())
}

#[test]
fn qualified_path_urn_roundtrip() {
    let urn = Urn::new(git2::Oid::zero().into()).with_path(reflike!("refs/rad/id"));
    let as_ref = Reference::try_from(&urn).unwrap();
    assert_eq!(urn, Urn::try_from(as_ref).unwrap())
}

#[test]
fn onelevel_path_urn_roundtrip() {
    let urn = Urn::new(git2::Oid::zero().into()).with_path(reflike!("rad/id"));
    let as_ref = Reference::try_from(&urn).unwrap();
    assert_eq!(
        urn.with_path(reflike!("refs/heads/rad/id")),
        Urn::try_from(as_ref).unwrap()
    )
}
