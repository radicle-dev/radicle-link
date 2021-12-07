// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use link_crypto::PeerId;
use link_identities::Urn;

/// A tracked entry for an `urn`.
pub enum Tracked<R, C> {
    /// Created when there was no peer to associate with the tracking action.
    Default { urn: Urn<R>, config: C },
    /// Created when there was a peer to associate with the tracking action.
    Peer {
        urn: Urn<R>,
        peer: PeerId,
        config: C,
    },
}

impl<R, C> Tracked<R, C> {
    pub fn urn(&self) -> &Urn<R> {
        match self {
            Self::Default { urn, .. } => urn,
            Self::Peer { urn, .. } => urn,
        }
    }

    pub fn peer_id(&self) -> Option<PeerId> {
        match self {
            Self::Default { .. } => None,
            Self::Peer { peer, .. } => Some(*peer),
        }
    }

    pub fn config(&self) -> &C {
        match self {
            Self::Default { config, .. } => config,
            Self::Peer { config, .. } => config,
        }
    }
}
