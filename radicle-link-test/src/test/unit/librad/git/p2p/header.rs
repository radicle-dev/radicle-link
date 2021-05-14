// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use git2::transport::Service as GitService;
use librad::{
    git::{p2p::header::Header, Urn},
    git_ext as ext,
    keys::SecretKey,
    peer::PeerId,
};

#[test]
fn roundtrip_unnonced() {
    let hdr = Header::new(
        GitService::UploadPackLs,
        Urn::new(ext::Oid::from(git2::Oid::zero())),
        PeerId::from(SecretKey::new()),
        None,
    );

    assert_eq!(hdr, hdr.to_string().parse::<Header<Urn>>().unwrap())
}

#[test]
fn roundtrip_nonced() {
    let hdr = Header::new(
        GitService::UploadPackLs,
        Urn::new(ext::Oid::from(git2::Oid::zero())),
        PeerId::from(SecretKey::new()),
        Some(69),
    );

    assert_eq!(hdr, hdr.to_string().parse::<Header<Urn>>().unwrap())
}
