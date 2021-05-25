// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryFrom as _;

use librad::{
    git_ext as ext,
    keys::SecretKey,
    peer::{conversion, PeerId},
};

use crate::roundtrip::*;

#[test]
fn test_default_encoding_roundtrip() {
    let peer_id1 = PeerId::from(SecretKey::new().public());
    let peer_id2 = PeerId::from_default_encoding(&peer_id1.default_encoding()).unwrap();

    assert_eq!(peer_id1, peer_id2)
}

#[test]
fn test_default_encoding_empty_input() {
    assert!(matches!(
        PeerId::from_default_encoding(""),
        Err(conversion::Error::UnexpectedInputLength(0))
    ))
}

#[test]
fn test_str_roundtrip() {
    str_roundtrip(PeerId::from(SecretKey::new().public()));
}

#[test]
fn test_cbor_roundtrip() {
    cbor_roundtrip(PeerId::from(SecretKey::new().public()))
}

#[test]
fn test_dns_name_roundtrip() {
    let peer_id1 = PeerId::from(SecretKey::new());
    let dns_name: webpki::DNSName = peer_id1.into();
    let peer_id2 = PeerId::try_from(dns_name.as_ref()).unwrap();

    assert_eq!(peer_id1, peer_id2)
}

#[test]
fn peerid_is_reflike() {
    let peer_id = PeerId::from(SecretKey::new());
    assert_eq!(
        &peer_id.to_string(),
        Into::<ext::RefLike>::into(&peer_id).as_str()
    )
}
