// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::{git::Urn, peer::PeerId};

use super::Attempts;
use serde::Serialize;

/// Events that can affect the state of the waiting room
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum Event {
    /// A request was created for a urn
    Created {
        /// The urn bein requested
        urn: Urn,
    },
    /// A query was initiated for a urn
    Queried {
        /// The urn bein queried
        urn: Urn,
    },
    /// A peer was found who claims to have a urn
    Found {
        /// The urn that was found
        urn: Urn,
        /// The peer who claims to have it
        peer: PeerId,
    },
    /// Cloning was initiated for a urn and peer
    Cloning {
        /// The urn we are cloning
        urn: Urn,
        /// The peer we are cloning from
        peer: PeerId,
    },
    /// Cloning failed for a urn and peer
    CloningFailed {
        /// The urn that failed
        urn: Urn,
        /// The peer we failed to clone from
        peer: PeerId,
        /// A description of why the cloning failed
        reason: String,
    },
    /// Cloning succeeded for a urn and peer
    Cloned {
        /// The urn we cloned
        urn: Urn,
        /// The peer we cloned from
        peer: PeerId,
    },
    /// A request for a urn was canceled
    Canceled {
        /// The urn that was canceled
        urn: Urn,
    },
    /// A request was removed from the waiting room
    Removed {
        /// The urn that was removed
        urn: Urn,
    },
    /// A request was timed out
    TimedOut {
        /// The urn that timed out
        urn: Urn,
        /// The attempts that were made before the timeout
        attempts: Attempts,
    },
    /// The waiting room was ticked - this is effectively a NOOP
    Tick,
}
