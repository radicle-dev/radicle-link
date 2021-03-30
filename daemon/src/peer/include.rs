// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! Handling of include files

use librad::{git::Urn, net::peer::Peer, signer::BoxedSigner};

use crate::state;

/// Update the include file for the given `RadUrn` and log the result.
pub async fn update(peer: Peer<BoxedSigner>, urn: Urn) {
    match state::update_include(&peer, urn.clone()).await {
        Ok(path) => log::debug!("Updated include file @ {}", path.display()),
        Err(err) => log::debug!("Failed to update include file for `{}`: {}", urn, err),
    }
}
