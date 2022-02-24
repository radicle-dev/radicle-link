// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::git::{local::url::LocalUrl, Urn};
use test_helpers::roundtrip;

#[test]
fn trip() {
    let url = LocalUrl::from(Urn::new(git2::Oid::zero().into()));
    roundtrip::str(url)
}
