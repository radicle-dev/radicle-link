// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

pub mod connection;
pub mod discovery;
pub mod peer;
pub mod protocol;
pub mod quic;
pub mod tls;
pub mod upgrade;
pub mod x509;

mod codec;

/// The network protocol version.
///
/// The version number denotes compatibility, and as such is subject to change
/// very infrequently: if two peers advertise the same version number, they MUST
/// be able to communicate with each other (ie. their implementations must be
/// compatible both forward and backward).
///
/// The protocol version is negotiated during the handshake via [ALPN], which
/// permits gradual rollout scenarios of major network upgrades.
///
/// For the negotiation of optional (compatible _per definitionem_) protocol
/// features, the [`PeerAdvertisement`] reserves space for advertising
/// [`info::Capability`]s.
///
/// [ALPN]: https://tools.ietf.org/html/rfc7301
pub const PROTOCOL_VERSION: u8 = 2;

/// Logical network.
///
/// This may be used to operate "devnets" without physical network isolation:
/// connections to and from peers advertising a non-matching [`Network`] SHALL
/// be rejected. Note, however, that joining multiple logical networks is not
/// precluded (inherent to "permissionless" protocols).
///
/// Custom network identifiers are part of the [ALPN] protocol identifier, and
/// should be kept short.
///
/// [ALPN]: https://tools.ietf.org/html/rfc7301
#[derive(Clone, Copy, Debug)]
pub enum Network {
    Main,
    Custom(&'static [u8]),
}

impl Default for Network {
    fn default() -> Self {
        Self::Main
    }
}
