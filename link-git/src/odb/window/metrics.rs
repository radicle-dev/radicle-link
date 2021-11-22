// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    marker::PhantomData,
    sync::atomic::{AtomicUsize, Ordering},
};

use tracing::trace;

pub struct StatsView {
    /// Total number of times the requested data was found in the cache.
    pub cache_hits: usize,
    /// Total number of times the requested data was not found in the cache.
    ///
    /// Note that a cache hit can occur after a miss if another thread was
    /// faster to fill in the missing entry. Thus, `cache_hits + cache_misses`
    /// does not necessarily sum up to the number of cache accesses.
    pub cache_misses: usize,
    /// Total number of times a pack file was attempted to be loaded from disk
    /// (incl. failed attempts).
    pub file_loads: usize,
    /// Total number of pack files the cache holds on to.
    pub open_files: usize,
}

#[derive(Default)]
pub struct Stats {
    hits: AtomicUsize,
    miss: AtomicUsize,
    load: AtomicUsize,
}

pub type Void = PhantomData<!>;

pub trait Metrics {
    type Snapshot;

    fn record_hit(&self);
    fn record_miss(&self);
    fn record_load(&self);

    fn snapshot(&self, open_files: usize) -> Self::Snapshot;
}

impl Metrics for Stats {
    type Snapshot = StatsView;

    fn record_hit(&self) {
        trace!("cache hit");
        self.hits.fetch_add(1, Ordering::Relaxed);
    }

    fn record_miss(&self) {
        trace!("cache miss");
        self.miss.fetch_add(1, Ordering::Relaxed);
    }

    fn record_load(&self) {
        trace!("pack load");
        self.load.fetch_add(1, Ordering::Relaxed);
    }

    fn snapshot(&self, open_files: usize) -> Self::Snapshot {
        StatsView {
            cache_hits: self.hits.load(Ordering::Relaxed),
            cache_misses: self.miss.load(Ordering::Relaxed),
            file_loads: self.load.load(Ordering::Relaxed),
            open_files,
        }
    }
}

impl Metrics for Void {
    type Snapshot = usize;

    fn record_hit(&self) {}
    fn record_miss(&self) {}
    fn record_load(&self) {}

    fn snapshot(&self, open_files: usize) -> Self::Snapshot {
        open_files
    }
}
