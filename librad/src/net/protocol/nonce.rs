// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    cmp::Reverse,
    hash::BuildHasherDefault,
    sync::Arc,
    time::{Duration, Instant},
};

use parking_lot::RwLock;
use priority_queue::PriorityQueue;
use rustc_hash::FxHasher;

pub type Nonce = u32;

/// Heap-like structure which keeps track of seen 32-bit nonces.
///
/// The elements are expired after `ttl`, while the capacity of the heap is
/// unconstrained.
#[derive(Clone)]
pub struct NonceBag {
    inner: Arc<RwLock<imp::NonceBag>>,
}

impl NonceBag {
    /// Create a new [`NonceBag`] which expires nonces after `ttl`.
    pub fn new(ttl: Duration) -> Self {
        Self {
            inner: Arc::new(RwLock::new(imp::NonceBag::new(ttl))),
        }
    }

    /// Returns `true` if the nonce is in the heap and not expired, `false`
    /// otherwise.
    pub fn contains(&self, nonce: &Nonce) -> bool {
        self.inner.read().contains(nonce)
    }

    /// Insert a nonce, or reset its expiry window if it is already in the heap.
    pub fn insert(&self, nonce: Nonce) {
        self.inner.write().insert(nonce)
    }
}

mod imp {
    use super::*;

    pub struct NonceBag {
        // TODO: Should probably use a `flurry` map instead, and compact
        // concurrently. Must understand the epoch GC first.
        queue: PriorityQueue<Nonce, Reverse<Instant>, BuildHasherDefault<FxHasher>>,
        tzero: Instant,
        ttl: Duration,
    }

    impl NonceBag {
        pub fn new(ttl: Duration) -> Self {
            Self {
                queue: PriorityQueue::with_capacity_and_hasher(1024, BuildHasherDefault::default()),
                tzero: Instant::now(),
                ttl,
            }
        }

        pub fn contains(&self, nonce: &Nonce) -> bool {
            matches!(self.queue.get(nonce), Some((_, Reverse(t))) if t.elapsed() < self.ttl)
        }

        pub fn insert(&mut self, nonce: Nonce) {
            if self.tzero.elapsed() >= self.ttl {
                self.compact()
            }
            self.queue.push_increase(nonce, Reverse(Instant::now()));
        }

        fn compact(&mut self) {
            let now = Instant::now();
            while let Some((n, Reverse(t))) = self.queue.pop() {
                if now.saturating_duration_since(t) < self.ttl {
                    self.queue.push(n, Reverse(t));
                    break;
                }
            }
            self.tzero = now;
            self.queue.shrink_to_fit();
        }
    }
}
