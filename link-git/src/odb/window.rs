// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{marker::PhantomData, sync::Arc};

use arc_swap::{ArcSwap, Guard};
use parking_lot::Mutex;

use super::pack;

mod metrics;
pub use metrics::{Metrics, Stats, StatsView, Void};

/// A threadsafe, shareable cache of packfiles.
pub trait Cache {
    type Stats;

    fn stats(&self) -> Self::Stats;

    fn get(&self, info: &pack::Info) -> Result<Arc<pack::Data>, pack::error::Data>;
}

impl<M, const B: usize, const S: usize> Cache for Fixed<M, B, S>
where
    M: Metrics,
{
    type Stats = M::Snapshot;

    fn stats(&self) -> Self::Stats {
        self.stats()
    }

    fn get(&self, info: &pack::Info) -> Result<Arc<pack::Data>, pack::error::Data> {
        self.get(info)
    }
}

/// 128 open files
pub type Small<S> = Fixed<S, 16, 8>;
/// 512 open files
pub type Medium<S> = Fixed<S, 32, 16>;
/// 1024 open files
pub type Large<S> = Fixed<S, 64, 16>;
/// 2048 open files
pub type XLarge<S> = Fixed<S, 128, 16>;

/// A fixed-size [`Cache`].
///
/// [`Fixed`] is essentially a very simple, fixed-capacity hashtable. When a
/// pack (data-) file is requested via [`Cache::get`], the file is loaded
/// (typically `mmap`ed) from disk if it is not already in the cache. Otherwise,
/// a pointer to the already loaded file is returned. Old entries are replaced
/// on an approximate LRU basis when the cache becomes full (this means that old
/// entries are **not** evicted when there is still space).
///
/// The implementation is a somewhat dumbed-down version of JGit's
/// `WindowCache`. The main differences are that the table buckets are of fixed
/// size (`SLOTS`), instead of a linked list. This means that the cache does not
/// allow to (temporarily) commit more entries than its nominal capacity.
///
/// Reading cached values is lock-free and mostly wait-free. Modifications are
/// guarded by locks on individual buckets; if a cache miss occurs, multiple
/// threads requesting the same entry will be blocked until one of them
/// succeeded loading the data and updating the cache. Writers will _not_,
/// however, contend with readers (unlike `RwLock`).
///
/// This favours usage patterns where different threads tend to request disjoint
/// sets of packfiles, and of course their hashes colliding relatively
/// infrequently.
pub struct Fixed<M, const BUCKETS: usize, const SLOTS: usize> {
    entries: [ArcSwap<[Option<Arc<pack::Data>>; SLOTS]>; BUCKETS],
    locks: [Mutex<()>; BUCKETS],
    stats: M,
}

trait AssertSendSync: Send + Sync {}
impl<M, const B: usize, const S: usize> AssertSendSync for Fixed<M, B, S> where M: Send + Sync {}

impl<M, const B: usize, const S: usize> AsRef<Fixed<M, B, S>> for Fixed<M, B, S> {
    fn as_ref(&self) -> &Fixed<M, B, S> {
        self
    }
}

impl<const B: usize, const S: usize> Default for Fixed<Void, B, S> {
    fn default() -> Self {
        Self {
            entries: [(); B].map(|_| ArcSwap::new(Arc::new([(); S].map(|_| None)))),
            locks: [(); B].map(|_| Mutex::new(())),
            stats: PhantomData,
        }
    }
}

impl<M, const B: usize, const S: usize> Fixed<M, B, S>
where
    M: Metrics,
{
    pub fn with_stats(self) -> Fixed<Stats, B, S> {
        self.with_metrics(Stats::default())
    }

    pub fn with_metrics<N: Metrics>(self, m: N) -> Fixed<N, B, S> {
        Fixed {
            entries: self.entries,
            locks: self.locks,
            stats: m,
        }
    }

    pub fn stats(&self) -> M::Snapshot {
        let open_files = self
            .entries
            .iter()
            .map(|bucket| bucket.load().iter().flatten().count())
            .sum();
        self.stats.snapshot(open_files)
    }

    pub fn get(&self, info: &pack::Info) -> Result<Arc<pack::Data>, pack::error::Data> {
        let idx = info.hash as usize % self.entries.len();

        let bucket = self.entries[idx].load();
        for entry in bucket.iter().flatten() {
            if entry.hash == info.hash {
                self.stats.record_hit();
                entry.hit();
                return Ok(Arc::clone(entry));
            }
        }
        drop(bucket);

        self.stats.record_miss();

        // Cache miss, try to load the data file
        let lock = self.locks[idx].lock();
        // Did someone else win the race for the lock?
        let bucket = self.entries[idx].load();
        for entry in bucket.iter().flatten() {
            if entry.hash == info.hash {
                self.stats.record_hit();
                entry.hit();
                return Ok(Arc::clone(entry));
            }
        }
        // No, proceed
        self.stats.record_load();
        let data = Arc::new(info.data()?);

        // Find an empty slot, or swap with the least popular
        let mut access = usize::MAX;
        let mut evict = 0;
        for (i, e) in bucket.iter().enumerate() {
            match e {
                Some(entry) => {
                    let hits = entry.hits();
                    if hits < access {
                        access = hits;
                        evict = i;
                    }
                },
                None => {
                    evict = i;
                    break;
                },
            }
        }
        let mut entries = Guard::into_inner(bucket);
        {
            // This costs `SLOTS` refcount increments if the slot is currently
            // borrowed.
            let mutti = Arc::make_mut(&mut entries);
            mutti[evict] = Some(Arc::clone(&data));
        }
        self.entries[idx].store(entries);
        drop(lock);

        data.hit();
        Ok(data)
    }
}
