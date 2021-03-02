// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::hash_map::RandomState,
    hash::{BuildHasher, Hash, Hasher as _},
    marker::PhantomData,
    sync::Arc,
};

use parking_lot::{ReentrantMutex, ReentrantMutexGuard};

/// A "keyed lock", which allows to acquire exclusive locks on arbitrary string
/// keys.
///
/// Implemented by hashing the string keys to a fixed set of mutexes allocated
/// upfront. This implies, of course, that two independent keys may actually
/// share a mutex.
#[derive(Clone)]
pub struct Klock<T, H = RandomState> {
    slots: Vec<Arc<ReentrantMutex<()>>>,
    hasher: H,
    _keys: PhantomData<T>,
}

/// RAII guard of a successfully acquired lock. The lock is released when this
/// value is dropped.
pub struct KlockGuard<'a>(ReentrantMutexGuard<'a, ()>);

impl<T> Klock<T, RandomState> {
    /// Create a new [`Klock`] with the default hasher and number of mutex
    /// slots.
    ///
    /// The number of mutex slots allocated is equal to [`num_cpus::get`].
    pub fn new() -> Self {
        Self::with_slots(num_cpus::get())
    }

    /// Create a new [`Klock`] with the default hasher and given number of mutex
    /// slots.
    pub fn with_slots(n: usize) -> Self {
        Self::with_slots_and_hasher(n, RandomState::default())
    }
}

impl<T, H> Klock<T, H>
where
    H: BuildHasher,
{
    /// Create a new [`Klock`] with the given number of mutex slots and given
    /// hasher.
    pub fn with_slots_and_hasher(n: usize, hasher: H) -> Self {
        let mut slots = Vec::with_capacity(n);
        for _ in 0..n {
            slots.push(Arc::new(ReentrantMutex::new(())))
        }

        Self {
            slots,
            hasher,
            _keys: PhantomData,
        }
    }
}

impl<T, H> Klock<T, H>
where
    T: Hash,
    H: BuildHasher,
{
    /// Acquires a lock for the given `key`, blocking the current thread until
    /// it is able to do so.
    ///
    /// This method is reentrant: the current thread may acquire more than one
    /// lock on the same or different keys.
    pub fn lock(&self, key: &T) -> KlockGuard {
        let mut hasher = self.hasher.build_hasher();
        key.hash(&mut hasher);
        let hash = hasher.finish();
        KlockGuard(self.slots[(hash % self.slots.len() as u64) as usize].lock())
    }

    /// Attempt to acquire a lock for the given `key`.
    ///
    /// Does not block if the lock could not be acquired, instead returns
    /// `None`.
    pub fn try_lock(&self, key: &T) -> Option<KlockGuard> {
        let mut hasher = self.hasher.build_hasher();
        key.hash(&mut hasher);
        let hash = hasher.finish();
        self.slots[(hash % self.slots.len() as u64) as usize]
            .try_lock()
            .map(KlockGuard)
    }
}

impl<T> Default for Klock<T> {
    fn default() -> Self {
        Self::new()
    }
}
