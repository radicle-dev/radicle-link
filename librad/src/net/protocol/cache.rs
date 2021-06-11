// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    ops::Deref,
    sync::{atomic::AtomicBool, Arc},
    thread,
    time::{Duration, Instant},
};

use parking_lot::{RwLock, RwLockReadGuard};
use thiserror::Error;

use crate::{
    git::{
        identities,
        storage::{self, watch},
    },
    identities::{xor, SomeUrn, Xor},
};

#[derive(Clone)]
pub struct Caches {
    pub urns: urns::Filter,
}

pub mod urns {
    use super::*;

    const DEBOUNCE_DELAY: Duration = Duration::from_millis(10);

    #[derive(Debug, Error)]
    #[non_exhaustive]
    pub enum Error {
        #[error(transparent)]
        Build(#[from] xor::BuildError<identities::Error>),

        #[error(transparent)]
        Watch(#[from] watch::Error),

        #[error(transparent)]
        Storage(#[from] storage::Error),
    }

    #[derive(Clone, Debug)]
    pub enum Event {
        Error(Arc<Box<dyn std::error::Error + Send + Sync + 'static>>),
        Rebuilt {
            built_in: Duration,
            len_old: usize,
            len_new: usize,
        },
    }

    #[derive(Clone, Copy, Debug, Default)]
    pub struct Stats {
        elements: usize,
        fingerprints: usize,
    }

    #[derive(Clone)]
    pub struct Filter {
        inner: Arc<RwLock<FilterInner>>,
        watch: storage::Watcher,
    }

    struct FilterInner {
        filter: Xor,
        elements: usize,
    }

    impl From<(Xor, usize)> for FilterInner {
        fn from((filter, elements): (Xor, usize)) -> Self {
            Self { filter, elements }
        }
    }

    impl Filter {
        pub fn new<F>(storage: storage::Storage, observe: F) -> Result<Self, Error>
        where
            F: Fn(Event) + Send + 'static,
        {
            let inner = {
                let inner = identities::any::xor_filter(&storage).map(FilterInner::from)?;
                Arc::new(RwLock::new(inner))
            };

            let (watch, events) = storage.watch().namespaces(DEBOUNCE_DELAY)?;
            thread::spawn({
                let filter = Arc::clone(&inner);
                move || recache_thread(storage, filter, events, observe)
            });

            Ok(Self { inner, watch })
        }

        pub fn contains(&self, urn: &SomeUrn) -> bool {
            self.inner.read().filter.contains(urn)
        }

        pub fn get(&self) -> impl Deref<Target = Xor> + '_ {
            RwLockReadGuard::map(self.inner.read(), |inner| &inner.filter)
        }

        /// The number of elements in the filter.
        pub fn len(&self) -> usize {
            self.inner.read().elements
        }

        pub fn is_empty(&self) -> bool {
            self.len() == 0
        }

        pub fn stats(&self) -> Stats {
            let inner = self.inner.read();
            Stats {
                elements: inner.elements,
                fingerprints: inner.filter.len(),
            }
        }
    }

    fn recache_thread<F>(
        storage: storage::Storage,
        filter: Arc<RwLock<FilterInner>>,
        events: impl Iterator<Item = watch::NamespaceEvent>,
        observe: F,
    ) where
        F: Fn(Event) + Send + 'static,
    {
        use std::sync::atomic::Ordering::*;

        let span = tracing::info_span!("recache-urns");
        let _guard = span.enter();

        let shutdown = Arc::new(AtomicBool::new(false));
        let rebuild = Arc::new(AtomicBool::new(false));

        let bob = thread::spawn({
            let span = span.clone();
            let shutdown = Arc::clone(&shutdown);
            let rebuild = Arc::clone(&rebuild);
            move || {
                let _guard = span.enter();
                'exit: loop {
                    if shutdown.load(Acquire) {
                        break;
                    }
                    while rebuild.fetch_and(false, Acquire) {
                        tracing::trace!("rebuilding xor filter...");
                        let len_old = filter.read().elements;
                        match build_filter(&storage) {
                            Err(e) => {
                                tracing::warn!(err = ?e, "error rebuilding xor filter");
                                observe(Event::Error(Arc::new(Box::new(e))))
                            },
                            Ok((new, dur)) => {
                                let len_new = new.elements;
                                tracing::trace!(
                                    len_old,
                                    len_new,
                                    "rebuilt xor filter in {:.2}s",
                                    dur.as_secs_f32()
                                );
                                let mut guard = filter.write();
                                *guard = new;
                                observe(Event::Rebuilt {
                                    built_in: dur,
                                    len_old,
                                    len_new,
                                });
                            },
                        }
                        if shutdown.load(Acquire) {
                            break 'exit;
                        }
                    }
                    thread::park()
                }
            }
        });

        for ev in events {
            tracing::trace!("new event: {:?}", ev);
            rebuild.store(true, Release);
            bob.thread().unpark()
        }

        shutdown.store(true, Release);
        bob.thread().unpark();
        bob.join().ok();
    }

    fn build_filter(
        storage: &storage::Storage,
    ) -> Result<(FilterInner, Duration), xor::BuildError<identities::Error>> {
        let start = Instant::now();
        identities::any::xor_filter(&storage).map(|res| (FilterInner::from(res), start.elapsed()))
    }
}
