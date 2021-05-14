// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use librad::{git::p2p::url::GitUrl, identities::git, keys::SecretKey, peer::PeerId};

use crate::roundtrip::str_roundtrip;

#[test]
fn test_str_roundtrip() {
    let url = GitUrl {
        local_peer: PeerId::from(SecretKey::new()),
        remote_peer: PeerId::from(SecretKey::new()),
        addr_hints: vec![
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 42)),
            SocketAddr::V6(SocketAddrV6::new(
                Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1),
                69,
                0,
                0,
            )),
        ],
        repo: git::Revision::from(git2::Oid::zero()),
        nonce: Some(42),
    };

    str_roundtrip(url)
}
