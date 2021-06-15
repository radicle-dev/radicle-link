// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    cmp::max,
    hash::Hash,
    mem,
    num::NonZeroUsize,
    sync::{
        atomic::{
            AtomicBool,
            Ordering::{Acquire, Release},
        },
        Arc,
    },
    thread::{self, Thread},
    time::Instant,
};

pub use governor::{
    clock::{Clock, DefaultClock},
    NotUntil,
    Quota,
};

pub type Direct = governor::RateLimiter<
    governor::state::direct::NotKeyed,
    governor::state::InMemoryState,
    governor::clock::DefaultClock,
>;

pub type Keyed<T> = governor::RateLimiter<
    T,
    governor::state::keyed::DashMapStateStore<T>,
    governor::clock::DefaultClock,
>;

#[derive(Clone)]
pub struct RateLimiter<T> {
    inner: Arc<T>,
    maint: Option<Maint>,
}

#[derive(Clone)]
struct Maint {
    thread: Thread,
    stop: Arc<AtomicBool>,
}

impl Drop for Maint {
    fn drop(&mut self) {
        if Arc::strong_count(&self.stop) == 2 {
            self.stop.store(true, Release);
            self.thread.unpark()
        }
    }
}

impl RateLimiter<Direct> {
    pub fn direct(quota: Quota) -> Self {
        Self {
            inner: Arc::new(governor::RateLimiter::direct(quota)),
            maint: None,
        }
    }

    pub fn check(&self) -> Result<(), NotUntil<<DefaultClock as Clock>::Instant>> {
        self.inner.check()
    }
}

impl<T> RateLimiter<Keyed<T>>
where
    T: Clone + Eq + Hash + Send + Sync + 'static,
{
    pub fn keyed(quota: Quota, mem: NonZeroUsize) -> Self {
        let inner = Arc::new(governor::RateLimiter::keyed(quota));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = thread::spawn({
            let maint_threshold = mem.get() / max(1, mem::size_of::<T>());
            let limiter = Arc::clone(&inner);
            let stop = Arc::clone(&stop);
            let span = tracing::debug_span!("rate-limiter-maint");
            move || {
                let _guard = span.enter();
                loop {
                    if stop.load(Acquire) {
                        tracing::debug!("stopping");
                        break;
                    }

                    if limiter.len() >= maint_threshold {
                        tracing::debug!(
                            "limiter is over threshold {}: {}",
                            maint_threshold,
                            limiter.len()
                        );
                        let start = Instant::now();
                        limiter.retain_recent();
                        tracing::debug!(
                            "sweeped limiter in {:.2}s, new len: {}",
                            start.elapsed().as_secs_f32(),
                            limiter.len()
                        );
                    }

                    thread::park()
                }
            }
        })
        .thread()
        .clone();

        Self {
            inner,
            maint: Some(Maint { thread, stop }),
        }
    }

    pub fn check_key(&self, k: &T) -> Result<(), NotUntil<<DefaultClock as Clock>::Instant>> {
        self.maint.as_ref().unwrap().thread.unpark();
        self.inner.check_key(k)
    }
}
