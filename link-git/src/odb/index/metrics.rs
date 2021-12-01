// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::sync::atomic::{AtomicUsize, Ordering};

pub struct StatsView {
    /// Total number of times a lookup was successful.
    pub hits: usize,
    /// Total number of times a lookup was unsuccessful.
    pub misses: usize,
    /// Total number of times an index was added explicitly via
    /// [`super::Shared::push`].
    pub pushes: usize,
    /// Total number of reloads via [`super::Shared::reload`].
    pub reloads: usize,
    /// Number of [`crate::odb::pack::Index`]es currently held.
    pub indices: usize,
}

#[derive(Default)]
pub struct Stats {
    hits: AtomicUsize,
    misses: AtomicUsize,
    pushes: AtomicUsize,
    reloads: AtomicUsize,
}

pub trait Metrics {
    type Snapshot;

    fn record_hit(&self);
    fn record_miss(&self);
    fn record_push(&self);
    fn record_reload(&self);

    fn snapshot(&self, indices: usize) -> Self::Snapshot;
}

impl Metrics for Stats {
    type Snapshot = StatsView;

    fn record_hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }

    fn record_miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }

    fn record_push(&self) {
        self.pushes.fetch_add(1, Ordering::Relaxed);
    }

    fn record_reload(&self) {
        self.reloads.fetch_add(1, Ordering::Relaxed);
    }

    fn snapshot(&self, indices: usize) -> Self::Snapshot {
        StatsView {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            pushes: self.pushes.load(Ordering::Relaxed),
            reloads: self.reloads.load(Ordering::Relaxed),
            indices,
        }
    }
}

impl Metrics for () {
    type Snapshot = ();

    fn record_hit(&self) {}
    fn record_miss(&self) {}
    fn record_push(&self) {}
    fn record_reload(&self) {}

    fn snapshot(&self, _: usize) -> Self::Snapshot {}
}
