// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Params {
    /// Maximum number of active connections.
    pub max_active: usize,
    /// Maximum number of passive connections.
    pub max_passive: usize,
    /// The number of hops a `ForwardJoin` or `Shuffle` should be propagated.
    pub active_random_walk_length: usize,
    /// The number of hops after which a `ForwardJoin` causes the sender to be
    /// inserted into the passive view.
    pub passive_random_walk_length: usize,
    /// The maximum number of peers to include in a shuffle.
    pub shuffle_sample_size: usize,
    /// Interval in which to perform a shuffle.
    pub shuffle_interval: Duration,
    /// Interval in which to attempt to promote a passive peer.
    pub promote_interval: Duration,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            // Note: this value is mentioned in the `bootstrap` param in `linkd-lib::args`.
            max_active: 5,
            max_passive: 30,
            active_random_walk_length: 6,
            passive_random_walk_length: 3,
            shuffle_sample_size: 7,
            shuffle_interval: Duration::from_secs(30),
            promote_interval: Duration::from_secs(30),
        }
    }
}
