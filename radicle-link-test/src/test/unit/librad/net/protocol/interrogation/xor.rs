// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use sha1::{Digest, Sha1};
use sized_vec::Vec as SVec;
use typenum::Unsigned;

use librad::{
    identities::{SomeUrn, Urn},
    net::protocol::interrogation::xor::*,
};

struct BuildUrn {
    hasher: Sha1,
}

impl BuildUrn {
    fn new() -> Self {
        Self {
            hasher: Sha1::new(),
        }
    }

    fn build(&mut self, v: &[u8]) -> SomeUrn {
        self.hasher.update(v);
        let digest = self.hasher.finalize_reset();
        let oid = git2::Oid::from_bytes(&digest).unwrap();
        SomeUrn::Git(Urn::new(oid.into()))
    }
}

#[test]
fn false_negatives() {
    let mut bob = BuildUrn::new();
    let urns = SVec::<MaxElements, _>::fill(|i| bob.build(&i.to_be_bytes()));
    let filter = Xor::from(&urns);

    for urn in urns {
        assert!(filter.contains(&urn))
    }
}

#[test]
fn false_positives() {
    let mut bob = BuildUrn::new();
    let urns = SVec::<MaxElements, _>::fill(|i| bob.build(&i.to_be_bytes()));
    let filter = Xor::from(&urns);

    let false_positives =
        SVec::<MaxElements, _>::fill(|_| bob.build(&rand::random::<usize>().to_be_bytes()))
            .iter()
            .filter(|urn| filter.contains(urn))
            .count();
    let rate: f64 = (false_positives * 100) as f64 / MaxElements::USIZE as f64;
    assert!(rate < 0.02, "False positive rate is {:?}", rate);
}
