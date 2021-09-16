// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{borrow::Cow, fmt::Display, str::FromStr};

pub mod codec;
pub mod connection;
pub mod discovery;
pub mod peer;
pub mod protocol;
pub mod quic;
pub mod tls;
pub mod upgrade;
pub mod x509;

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
/// features, the [`protocol::PeerAdvertisement`] reserves space for advertising
/// [`protocol::Capability`]s.
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Network {
    Main,
    Custom(Cow<'static, [u8]>),
}

impl Default for Network {
    fn default() -> Self {
        Self::Main
    }
}

impl Display for Network {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl FromStr for Network {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = s.as_bytes();
        if bytes.len() > 32 {
            Err("network name should not exceed 32 bytes")
        } else {
            Ok(Self::Custom(Cow::Owned(bytes.to_owned())))
        }
    }
}
