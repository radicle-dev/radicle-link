// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! Emit `Have`s and `Want`s on the network.

use librad::{
    git::Urn,
    git_ext::Oid,
    net::{
        peer::Peer,
        protocol::gossip::{Payload, Rev},
    },
    peer::PeerId,
    signer::BoxedSigner,
};

/// Announce a new rev for the `urn`.
pub fn announce(peer: &Peer<BoxedSigner>, urn: &Urn, rev: Option<Oid>) {
    match peer.announce(Payload {
        urn: urn.clone(),
        rev: rev.map(|rev| Rev::Git(rev.into())),
        origin: None,
    }) {
        Ok(()) => log::trace!("successfully announced for urn=`{}`, rev=`{:?}`", urn, rev),
        Err(_payload) => log::warn!("failed to announce for urn=`{}`, rev=`{:?}`", urn, rev),
    }
}

/// Emit a [`Payload`] request for the given `urn`.
pub fn query(peer: &Peer<BoxedSigner>, urn: &Urn, origin: Option<PeerId>) {
    match peer.query(Payload {
        urn: urn.clone(),
        rev: None,
        origin,
    }) {
        Ok(()) => log::trace!(
            "successfully queried for urn=`{}`, origin=`{:?}`",
            urn,
            origin
        ),
        Err(_payload) => log::warn!("failed to query for urn=`{}`, origin=`{:?}`", urn, origin),
    };
}
