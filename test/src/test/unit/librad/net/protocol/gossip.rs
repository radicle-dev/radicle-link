// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::{git::Urn, git_ext, keys::SecretKey, net::protocol::gossip::*, peer::PeerId};

use crate::roundtrip::*;

lazy_static! {
    static ref OID: git2::Oid =
        git2::Oid::hash_object(git2::ObjectType::Commit, b"chrzbrr").unwrap();
}

#[test]
fn roundtrip_rev() {
    cbor_roundtrip(Rev::Git(*OID));
}

#[test]
fn roundtrip_payload() {
    let payload = Payload {
        urn: Urn::new(git_ext::Oid::from(git2::Oid::zero())),
        rev: Some(Rev::Git(*OID)),
        origin: Some(PeerId::from(SecretKey::new())),
    };

    cbor_roundtrip(payload)
}
