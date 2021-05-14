// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use pretty_assertions::assert_eq;

use librad::{keys::SecretKey, net::x509::Certificate, peer::PeerId};

lazy_static! {
    static ref KEY: SecretKey = SecretKey::from_seed([
        251, 165, 81, 85, 1, 6, 204, 204, 106, 221, 120, 177, 80, 197, 248, 32, 153, 32, 10, 81,
        157, 238, 183, 70, 192, 158, 141, 52, 194, 41, 230, 215
    ]);
}

#[test]
fn roundtrip() {
    let cert = Certificate::generate(&*KEY).unwrap();
    assert_eq!(cert, Certificate::from_der(&cert.to_der()).unwrap())
}

#[test]
fn get_peer_id() {
    let cert = Certificate::generate(&*KEY).unwrap();
    let cert2 = Certificate::from_der(&cert.to_der()).unwrap();

    let peer_id = PeerId::from(&*KEY);
    assert_eq!(cert.peer_id_ref(), cert2.peer_id_ref());
    assert_eq!(&peer_id, cert.peer_id_ref());
}
