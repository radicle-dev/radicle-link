// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    convert::TryFrom as _,
    net::SocketAddr,
    panic,
    time::{Duration, Instant},
};

use futures::stream::FuturesUnordered;

use crate::{
    bloom,
    git::{
        identities,
        replication,
        storage::{pool::PooledStorage, Storage},
    },
    identities::SomeUrn,
    PeerId,
};

pub mod error;
pub mod rpc;
pub use rpc::{Request, Response};

pub const MAX_OFFER_TOTAL: usize = 10_000;

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub sync_period: Duration,
    pub bloom_filter_accuracy: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            sync_period: Duration::from_secs(5 * 60),
            bloom_filter_accuracy: 0.0001,
        }
    }
}

pub struct State {
    config: Config,
    deadline: Instant,
    snapshot: Option<bloom::BloomFilter<SomeUrn>>,
}

impl State {
    pub fn new(storage: &Storage, config: Config) -> Result<Self, error::State> {
        let snapshot = identities::any::bloom(storage, config.bloom_filter_accuracy)?;
        let deadline = Instant::now() + config.sync_period;
        Ok(Self {
            config,
            deadline,
            snapshot,
        })
    }

    pub fn reset(&mut self, storage: &Storage) -> Result<(), error::State> {
        self.snapshot = identities::any::bloom(storage, self.config.bloom_filter_accuracy)?;
        self.deadline = Instant::now() + self.config.sync_period;

        Ok(())
    }

    pub fn should_sync(&self) -> bool {
        self.snapshot.is_some() && Instant::now() > self.deadline
    }

    pub fn deadline(&self) -> Instant {
        self.deadline
    }

    pub fn snapshot(&self) -> Option<&bloom::BloomFilter<SomeUrn>> {
        self.snapshot.as_ref()
    }
}

#[tracing::instrument(skip(storage), err)]
pub async fn handle_request(
    storage: impl AsRef<Storage>,
    request: Request,
) -> Result<impl Iterator<Item = Response>, error::Request> {
    let Request::ListNamespaces { filter } = request;
    let bloom = filter
        .map(bloom::BloomFilter::try_from)
        .transpose()
        .map_err(error::Request::Bloom)?;
    let offers = self::offer_namespaces(storage, bloom).await?;

    Ok(offers
        .into_iter()
        .map(|batch| Response::OfferNamespaces { batch }))
}

#[tracing::instrument(skip(storage))]
pub fn handle_response<S>(
    storage: &S,
    response: Response,
    remote_id: PeerId,
    remote_addr: SocketAddr,
) -> impl futures::Stream<Item = Result<SomeUrn, error::Response>> + '_
where
    S: PooledStorage + Send + Sync + 'static,
{
    let Response::OfferNamespaces { batch } = response;
    batch
        .into_iter()
        .map(move |urn| async move {
            let SomeUrn::Git(gurn) = urn.clone();
            let storage = storage.get().await?;
            let task = tokio::task::spawn_blocking(move || {
                replication::replicate(storage.as_ref(), None, gurn, remote_id, Some(remote_addr))
            });

            match task.await {
                Err(e) => {
                    if let Ok(panicked) = e.try_into_panic() {
                        panic::resume_unwind(panicked)
                    } else {
                        Err(error::Response::Cancelled)
                    }
                },

                Ok(res) => Ok(res.map(|()| urn)?),
            }
        })
        .collect::<FuturesUnordered<_>>()
}

// FIXME: There is no chunking method on iterators, due to lifetime issues.
// Since we have owned items, that shouldn't actually bother us, but we need to
// roll our own iterator to make this function stream (which we want!)
async fn offer_namespaces(
    storage: impl AsRef<Storage>,
    filter: Option<bloom::BloomFilter<SomeUrn>>,
) -> Result<Vec<rpc::Offer>, error::Offer> {
    let urns = identities::any::list_urns(storage.as_ref())?
        .filter_map(|res| match res {
            Err(e) => Some(Err(e)),
            Ok(urn) => {
                let urn = SomeUrn::Git(urn);
                filter
                    .as_ref()
                    .map(|bloom| bloom.contains(&urn))
                    .unwrap_or(true)
                    .then_some(Ok(urn))
            },
        })
        .collect::<Result<Vec<_>, _>>()?;

    let offers = urns
        .chunks(rpc::MAX_OFFER_BATCH_SIZE)
        .map(|chunk| {
            rpc::Offer::try_from(chunk.to_vec()).expect("chunk size equals batch size. qed")
        })
        .collect::<Vec<_>>();

    Ok(offers)
}
