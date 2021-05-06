// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::iter;

use multibase::{self, Base};

use librad::keys::*;

use crate::roundtrip::*;

const DATA_TO_SIGN: &[u8] = b"alors monsieur";

#[test]
fn test_sign_verify_via_signature() {
    let key = SecretKey::new();
    let sig = key.sign(&DATA_TO_SIGN);
    assert!(sig.verify(&DATA_TO_SIGN, &key.public()))
}

#[test]
fn test_sign_verify_via_pubkey() {
    let key = SecretKey::new();
    let sig = key.sign(&DATA_TO_SIGN);
    assert!(key.public().verify(&sig, &DATA_TO_SIGN))
}

#[test]
fn test_public_key_json() {
    json_roundtrip(SecretKey::new().public())
}

#[test]
fn test_public_key_cbor() {
    cbor_roundtrip(SecretKey::new().public())
}

#[test]
fn test_public_key_deserialize_wrong_version() {
    let pk = SecretKey::new().public();
    let ser = multibase::encode(
        Base::Base32Z,
        iter::once(&1)
            .chain(pk.as_ref())
            .cloned()
            .collect::<Vec<u8>>(),
    );
    assert!(serde_json::from_str::<PublicKey>(&ser).is_err())
}

#[test]
fn test_signature_json() {
    json_roundtrip(SecretKey::new().sign(&DATA_TO_SIGN))
}

#[test]
fn test_signature_cbor() {
    cbor_roundtrip(SecretKey::new().sign(&DATA_TO_SIGN))
}

#[test]
fn test_signature_deserialize_wrong_version() {
    let sig = SecretKey::new().sign(&DATA_TO_SIGN);
    let ser = multibase::encode(
        Base::Base32Z,
        iter::once(&1)
            .chain(&<[u8; 64]>::from(sig)[..])
            .cloned()
            .collect::<Vec<u8>>(),
    );
    assert!(serde_json::from_str::<Signature>(&ser).is_err())
}
