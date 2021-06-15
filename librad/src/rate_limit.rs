// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    cmp::max,
    hash::Hash,
    mem,
    num::NonZeroUsize,
    sync::{Arc, Weak},
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
    maint: Option<Thread>,
}

impl<T> Drop for RateLimiter<T> {
    fn drop(&mut self) {
        if let Some(t) = self.maint.as_ref() {
            t.unpark()
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
        let maint = thread::spawn({
            let maint_threshold = mem.get() / max(1, mem::size_of::<T>());
            let limiter = Arc::downgrade(&inner);
            let span = tracing::debug_span!("rate-limiter-maint");
            move || {
                let _guard = span.enter();
                loop {
                    match Weak::upgrade(&limiter) {
                        None => {
                            tracing::debug!("limiter gone");
                            break;
                        },
                        Some(lim) => {
                            if lim.len() >= maint_threshold {
                                tracing::debug!(
                                    "limiter is over threshold {}: {}",
                                    maint_threshold,
                                    lim.len()
                                );
                                let start = Instant::now();
                                lim.retain_recent();
                                tracing::debug!(
                                    "sweeped limiter in {:.2}s, new len: {}",
                                    start.elapsed().as_secs_f32(),
                                    lim.len()
                                );
                            }
                        },
                    }

                    thread::park()
                }
            }
        })
        .thread()
        .clone();

        Self {
            inner,
            maint: Some(maint),
        }
    }

    pub fn check_key(&self, k: &T) -> Result<(), NotUntil<<DefaultClock as Clock>::Instant>> {
        self.maint.as_ref().unwrap().unpark();
        self.inner.check_key(k)
    }
}
