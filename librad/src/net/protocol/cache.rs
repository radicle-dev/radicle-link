// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{ops::Deref, sync::Arc, thread, time::Instant};

use parking_lot::RwLock;
use thiserror::Error;

use crate::{
    git::{identities, storage},
    identities::{xor, SomeUrn, Xor},
};

#[derive(Clone)]
pub struct Caches {
    pub urns: urns::Filter,
}

pub mod urns {
    use super::*;

    #[derive(Debug, Error)]
    #[non_exhaustive]
    pub enum Error {
        #[error(transparent)]
        Build(#[from] xor::BuildError<identities::Error>),

        #[error(transparent)]
        Watch(#[from] storage::watch::Error),

        #[error(transparent)]
        Storage(#[from] storage::Error),
    }

    #[derive(Clone)]
    pub struct Filter {
        inner: Arc<RwLock<Xor>>,
        watch: storage::Watcher,
    }

    impl Filter {
        pub fn new(storage: storage::Storage) -> Result<Self, Error> {
            let xor = identities::any::xor_filter(&storage)?;
            let inner = Arc::new(RwLock::new(xor));

            let (watch, events) = storage.watch().refs()?;
            thread::spawn({
                let filter = Arc::clone(&inner);
                move || recache_thread(storage, filter, events)
            });

            Ok(Self { inner, watch })
        }

        pub fn contains(&self, urn: &SomeUrn) -> bool {
            self.inner.read().contains(urn)
        }

        pub fn get(&self) -> impl Deref<Target = Xor> + '_ {
            self.inner.read()
        }
    }

    fn recache_thread(
        storage: storage::Storage,
        filter: Arc<RwLock<Xor>>,
        events: impl Iterator<Item = storage::RefsEvent>,
    ) {
        let span = tracing::info_span!("recache-urns");
        let _guard = span.enter();
        for evt in events {
            if is_urn_event(evt) {
                let start = Instant::now();
                match identities::any::xor_filter(&storage) {
                    Err(e) => {
                        tracing::warn!(err = ?e, "error rebuilding xor filter")
                    },
                    Ok(xor) => {
                        tracing::info!(
                            "rebuilt xor filter in {:.2}s",
                            start.elapsed().as_secs_f32()
                        );
                        let mut guard = filter.write();
                        *guard = xor;
                    },
                }
            }
        }
    }

    fn is_urn_event(storage::RefsEvent { path, kind }: storage::RefsEvent) -> bool {
        matches!(
            kind,
            storage::RefsEventKind::Create | storage::RefsEventKind::Remove
        ) && path.starts_with("refs/namespaces")
            && path.iter().take(4).count() == 3
    }
}
