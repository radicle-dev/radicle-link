// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{cmp::max, hash::Hash, mem, num::NonZeroUsize, sync::Arc, thread, time::Instant};

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
    need_maint: Option<crossbeam_channel::Sender<()>>,
}

impl RateLimiter<Direct> {
    pub fn direct(quota: Quota) -> Self {
        Self {
            inner: Arc::new(governor::RateLimiter::direct(quota)),
            need_maint: None,
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
        let maint_threshold = mem.get() / max(1, mem::size_of::<T>());
        let (tx, rx) = crossbeam_channel::unbounded();
        thread::spawn({
            let limiter = Arc::clone(&inner);
            let span = tracing::debug_span!("rate-limiter-maint");
            move || {
                let _guard = span.enter();
                for _ in rx {
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
                }
            }
        });

        Self {
            inner,
            need_maint: Some(tx),
        }
    }

    pub fn check_key(&self, k: &T) -> Result<(), NotUntil<<DefaultClock as Clock>::Instant>> {
        self.need_maint.as_ref().unwrap().send(()).ok();
        self.inner.check_key(k)
    }
}
