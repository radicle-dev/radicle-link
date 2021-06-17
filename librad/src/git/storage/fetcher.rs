// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::BTreeMap,
    convert::TryFrom,
    hash::BuildHasherDefault,
    net::SocketAddr,
    time::Duration,
};

use dashmap::DashMap;
use git_ext::RefLike;
use rustc_hash::FxHasher;
use url::Url;

use super::{PoolError, Storage};
use crate::{
    executor,
    git::{
        fetch::{self, FetchResult, Fetchspecs, RemoteHeads},
        p2p::url::GitUrlRef,
        Urn,
    },
    identities::{self, git::Revision},
    PeerId,
};

#[derive(Clone, Debug)]
pub struct Info {
    pub urn: Urn,
    pub remote_peer: PeerId,
}

/// Tracks concurrent fetchers of the same [`Urn`].
///
/// Whenever multiple [`Storage`] instances are in use simultaneously (such as
/// in a [`super::Pool`]), they MUST share a single instance of [`Fetchers`].
///
/// Create via [`Default`].
#[derive(Clone, Default)]
pub struct Fetchers(DashMap<Urn, Info, BuildHasherDefault<FxHasher>>);

/// [`Storage`]-specific [`fetch::Fetcher`] impl.
pub struct Fetcher<'a> {
    reg: &'a Fetchers,
    inner: imp::Fetcher<'a>,
}

impl Drop for Fetcher<'_> {
    fn drop(&mut self) {
        self.reg.0.remove(&self.inner.info().urn);
    }
}

impl<'a> fetch::Fetcher for Fetcher<'a> {
    type Error = <imp::Fetcher<'a> as fetch::Fetcher>::Error;
    type PeerId = <imp::Fetcher<'a> as fetch::Fetcher>::PeerId;
    type UrnId = <imp::Fetcher<'a> as fetch::Fetcher>::UrnId;

    fn urn(&self) -> &identities::Urn<Self::UrnId> {
        self.inner.urn()
    }

    fn remote_peer(&self) -> &Self::PeerId {
        self.inner.remote_peer()
    }

    fn remote_heads(&self) -> &fetch::RemoteHeads {
        self.inner.remote_heads()
    }

    fn fetch(
        &mut self,
        specs: fetch::Fetchspecs<Self::PeerId, Self::UrnId>,
    ) -> Result<fetch::FetchResult, Self::Error> {
        self.inner.fetch(specs)
    }
}

/// Types which can create a [`Fetcher`].
pub trait BuildFetcher {
    type Error: std::error::Error + Send + Sync + 'static;

    fn urn(&self) -> &Urn;
    fn remote_peer(&self) -> &PeerId;

    fn build_fetcher<'a>(
        &self,
        storage: &'a Storage,
    ) -> Result<Result<Fetcher<'a>, Info>, Self::Error>;
}

/// A [`BuildFetcher`] which creates [`Fetcher`]s which use the peer-to-peer
/// network.
#[derive(Debug, Clone)]
pub struct PeerToPeer {
    pub urn: Urn,
    pub remote_peer: PeerId,
    pub addr_hints: Vec<SocketAddr>,
    pub nonced: bool,
}

impl PeerToPeer {
    pub fn new<Addrs>(urn: Urn, remote_peer: PeerId, addr_hints: Addrs) -> Self
    where
        Addrs: IntoIterator<Item = SocketAddr>,
    {
        Self {
            urn: Urn::new(urn.id),
            remote_peer,
            addr_hints: addr_hints.into_iter().collect(),
            nonced: true,
        }
    }

    pub fn nonced(self, doit: bool) -> Self {
        Self {
            nonced: doit,
            ..self
        }
    }

    pub fn build<'a>(
        &self,
        storage: &'a Storage,
    ) -> Result<Result<Fetcher<'a>, Info>, git2::Error> {
        let nonce = if self.nonced {
            Some(rand::random())
        } else {
            None
        };

        let url = GitUrlRef {
            local_peer: &PeerId::from_signer(storage.signer()),
            remote_peer: &self.remote_peer,
            repo: &self.urn.id,
            addr_hints: &self.addr_hints,
            nonce: nonce.as_ref(),
        };
        AnyUrl {
            urn: self.urn.clone(),
            remote_peer: self.remote_peer,
            url: Url::from(url),
        }
        .build(storage)
    }
}

impl BuildFetcher for PeerToPeer {
    type Error = git2::Error;

    fn urn(&self) -> &Urn {
        &self.urn
    }

    fn remote_peer(&self) -> &PeerId {
        &self.remote_peer
    }

    fn build_fetcher<'a>(
        &self,
        storage: &'a Storage,
    ) -> Result<Result<Fetcher<'a>, Info>, Self::Error> {
        self.build(storage)
    }
}

/// A [`BuildFetcher`] which creates [`Fetcher`]s from any [`Url`] for which a
/// transport exists.
///
/// **Note** that this crate disables all features of the `git2` create, which
/// means that, by default, HTTPS and SSH transports are not accessible.
pub struct AnyUrl {
    pub urn: Urn,
    pub remote_peer: PeerId,
    pub url: Url,
}

impl AnyUrl {
    pub fn build<'a>(
        &self,
        storage: &'a Storage,
    ) -> Result<Result<Fetcher<'a>, Info>, git2::Error> {
        use dashmap::mapref::entry::Entry;

        let fetchers = storage.fetchers();
        // The joy of concurrent maps:
        //
        // We cannot place a lock on just the URN key during construction of the
        // fetcher. For dashmap, doing so would mean that concurrent calls using
        // a different URN which _happens_ to be mapped to the same shard would
        // block.
        //
        // Instead, return fast if the slot is already taken, otherwise get a
        // fetcher, and then check again. Worst case we're wasting some bandwidth.
        //
        if let Some(inflight) = fetchers.0.get(&self.urn) {
            return Ok(Err(inflight.value().clone()));
        }

        let fetcher = imp::Fetcher::with_url(
            storage,
            self.url.clone(),
            self.urn.clone(),
            self.remote_peer,
        )?;
        match fetchers.0.entry(self.urn.clone()) {
            Entry::Vacant(entry) => {
                let info = fetcher.info();
                entry.insert(Info {
                    urn: info.urn.clone(),
                    remote_peer: info.remote_peer,
                });
                Ok(Ok(Fetcher {
                    reg: fetchers,
                    inner: fetcher,
                }))
            },

            Entry::Occupied(entry) => Ok(Err(entry.get().clone())),
        }
    }
}

impl BuildFetcher for AnyUrl {
    type Error = git2::Error;

    fn urn(&self) -> &Urn {
        &self.urn
    }

    fn remote_peer(&self) -> &PeerId {
        &self.remote_peer
    }

    fn build_fetcher<'a>(
        &self,
        storage: &'a Storage,
    ) -> Result<Result<Fetcher<'a>, Info>, Self::Error> {
        self.build(storage)
    }
}

pub mod error {
    use super::*;
    use crate::executor::{Cancelled, JoinError};
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum Retrying<E: std::error::Error + Send + Sync + 'static> {
        #[error("fetch of {urn} from {remote_peer} already in-flight")]
        Concurrent { urn: Urn, remote_peer: PeerId },

        #[error(transparent)]
        Task(Cancelled),

        #[error("unable to create fetcher")]
        MkFetcher(#[source] E),

        #[error(transparent)]
        Pool(#[from] super::PoolError),
    }

    impl<E> From<JoinError> for Retrying<E>
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        fn from(e: JoinError) -> Self {
            Self::Task(e.into_cancelled())
        }
    }

    #[derive(Debug, Error)]
    pub enum FetchError {
        #[error("Fetch limit {limit} exceeded, fetched: {amount_fetched} from {remote}")]
        FetchLimitExceeded {
            limit: usize,
            amount_fetched: usize,
            remote: PeerId,
            fetchspecs: Fetchspecs<PeerId, Revision>,
            refspecs: Vec<String>,
        },
        #[error(transparent)]
        Git(#[from] git2::Error),
    }
}

/// Try to acquire a [`Fetcher`] in an async context, and run the provided
/// closure using it.
///
/// If a concurrent fetch for the same [`Urn`] and **a different** remote peer
/// is currently in-flight, this function retries (with backoff) for at most the
/// [`Duration`] given by `timeout`.
///
/// If the remote peer is the same, or `timeout` elapses, an error is returned
/// and the closure is **not** invoked.
///
/// # Fairness
///
/// On every attempt to acquire a [`Fetcher`], a new [`Storage`] is acquired
/// from the pool, ie. the resource is not held on to across sleeps. The backoff
/// strategy increases the sleep interval after each attempt, so is biased
/// towards more recent requests for the same resource.
pub async fn retrying<P, B, E, F, A>(
    spawner: &executor::Spawner,
    pool: &P,
    builder: B,
    timeout: Duration,
    f: F,
) -> Result<A, error::Retrying<E>>
where
    P: super::Pooled + Send + 'static,
    B: BuildFetcher<Error = E> + Clone + Send + 'static,
    E: std::error::Error + Send + Sync + 'static,
    F: Fn(&Storage, Fetcher) -> A + Send + Sync + 'static,
    A: Send + 'static,
{
    use backoff::{backoff::Backoff as _, ExponentialBackoff};

    enum Inner<B, F, E>
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Fatal(error::Retrying<E>),
        Retry { b: B, f: F, err: error::Retrying<E> },
    }

    async fn go<P, B, F, A, E>(
        spawner: &executor::Spawner,
        pool: &P,
        builder: B,
        f: F,
    ) -> Result<A, Inner<B, F, E>>
    where
        P: super::Pooled + Send + 'static,
        B: BuildFetcher<Error = E> + Send + 'static,
        F: Fn(&Storage, Fetcher) -> A + Send + Sync + 'static,
        E: std::error::Error + Send + Sync + 'static,
        A: Send + 'static,
    {
        let storage = pool
            .get()
            .await
            .map_err(error::Retrying::from)
            .map_err(Inner::Fatal)?;
        spawner.block_in_place(move || {
            let fetcher = builder
                .build_fetcher(&storage)
                .map_err(error::Retrying::MkFetcher)
                .map_err(Inner::Fatal)?;

            match fetcher {
                Ok(fetcher) => Ok(f(&storage, fetcher)),
                Err(info) => {
                    let keep_going = &info.remote_peer != builder.remote_peer();
                    let err = error::Retrying::Concurrent {
                        urn: info.urn,
                        remote_peer: info.remote_peer,
                    };

                    if keep_going {
                        Err(Inner::Retry { b: builder, f, err })
                    } else {
                        Err(Inner::Fatal(err))
                    }
                },
            }
        })
    }

    let mut policy = ExponentialBackoff {
        current_interval: Duration::from_secs(1),
        initial_interval: Duration::from_secs(1),
        max_interval: Duration::from_secs(5),
        max_elapsed_time: Some(timeout),
        ..Default::default()
    };
    let mut fut = go(spawner, pool, builder, f);
    loop {
        match fut.await {
            Err(Inner::Retry { b, f, err }) => match policy.next_backoff() {
                None => return Err(err),
                Some(next) => {
                    tracing::info!(
                        urn = %b.urn(),
                        remote_peer = %b.remote_peer(),
                        "unable to obtain fetcher, retrying in {:?}", next
                    );
                    tokio::time::sleep(next).await;
                    fut = go(spawner, pool, b, f);
                    continue;
                },
            },
            Err(Inner::Fatal(e)) => return Err(e),
            Ok(a) => return Ok(a),
        }
    }
}

mod imp {
    use super::*;

    pub struct Info {
        pub urn: Urn,
        pub remote_peer: PeerId,
        pub remote_heads: RemoteHeads,
    }

    pub struct Fetcher<'a> {
        info: Info,
        remote: git2::Remote<'a>,
    }

    impl<'a> Fetcher<'a> {
        pub fn with_url(
            storage: &'a Storage,
            url: Url,
            urn: Urn,
            remote_peer: PeerId,
        ) -> Result<Self, git2::Error> {
            let mut remote = match storage.as_raw().remote_anonymous(url.as_str()) {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!(?url, err=?e, "Error opening remote");
                    return Err(e);
                },
            };
            remote.connect(git2::Direction::Fetch)?;
            let remote_heads = remote
                .list()?
                .iter()
                .filter_map(|remote_head| match remote_head.symref_target() {
                    Some(_) => None,
                    None => match RefLike::try_from(remote_head.name()) {
                        Ok(refname) => Some((refname, remote_head.oid().into())),
                        Err(e) => {
                            tracing::warn!("invalid refname `{}`: {}", remote_head.name(), e);
                            None
                        },
                    },
                })
                .collect::<BTreeMap<_, _>>()
                .into();
            let info = Info {
                urn,
                remote_peer,
                remote_heads,
            };

            Ok(Self { info, remote })
        }

        pub fn info(&self) -> &Info {
            &self.info
        }

        #[tracing::instrument(skip(self))]
        pub fn fetch(
            &mut self,
            fetchspecs: Fetchspecs<PeerId, Revision>,
        ) -> Result<FetchResult, error::FetchError> {
            let mut updated_tips = BTreeMap::new();
            {
                let limit = fetchspecs.fetch_limit();
                let refspecs = fetchspecs
                    .refspecs(
                        &self.info.urn,
                        self.info.remote_peer,
                        &self.info.remote_heads,
                    )
                    .into_iter()
                    .map(|spec| spec.to_string())
                    .collect::<Vec<_>>();
                tracing::trace!("{:?}", refspecs);

                let mut callbacks = git2::RemoteCallbacks::new();
                let mut excessive_transfer_bytes: Option<usize> = None;
                callbacks.transfer_progress(|prog| {
                    let received_bytes = prog.received_bytes();
                    tracing::trace!("Fetch: received {} bytes", received_bytes);
                    if received_bytes > limit {
                        tracing::error!("Fetch: exceeded {} bytes", limit);
                        excessive_transfer_bytes = Some(received_bytes);
                        false
                    } else {
                        true
                    }
                });

                // FIXME: Using `download` + `update_tips` is preferable here because
                // `fetch` is a composition of `connect`, `download` + `update_tips`,
                // which means we're transmitting the refs advertisement multiple
                // times redundantly.
                //
                // Upstream issue: https://github.com/libgit2/libgit2/issues/5799.
                callbacks.update_tips(|name, old, new| {
                    tracing::debug!("Fetch: updating tip {}: {} -> {}", name, old, new);
                    match RefLike::try_from(name) {
                        Ok(refname) => {
                            updated_tips.insert(refname, new.into());
                        },
                        Err(e) => tracing::warn!("invalid refname `{}`: {}", name, e),
                    }

                    true
                });

                let res = self.remote.fetch(
                    &refspecs,
                    Some(
                        git2::FetchOptions::new()
                            .prune(git2::FetchPrune::Off)
                            .update_fetchhead(false)
                            .download_tags(git2::AutotagOption::None)
                            .remote_callbacks(callbacks),
                    ),
                    None,
                );

                if let Some(excessive_transfer_bytes) = excessive_transfer_bytes {
                    Err(error::FetchError::FetchLimitExceeded {
                        limit,
                        remote: self.info.remote_peer,
                        fetchspecs,
                        amount_fetched: excessive_transfer_bytes,
                        refspecs,
                    })
                } else {
                    res.map_err(|e| e.into())
                }?;
            }

            Ok(FetchResult { updated_tips })
        }
    }

    impl fetch::Fetcher for Fetcher<'_> {
        type Error = error::FetchError;
        type PeerId = PeerId;
        type UrnId = Revision;

        fn urn(&self) -> &Urn {
            &self.info.urn
        }

        fn remote_peer(&self) -> &PeerId {
            &self.info.remote_peer
        }

        fn remote_heads(&self) -> &fetch::RemoteHeads {
            &self.info.remote_heads
        }

        fn fetch(
            &mut self,
            fetchspecs: Fetchspecs<Self::PeerId, Self::UrnId>,
        ) -> Result<FetchResult, Self::Error> {
            self.fetch(fetchspecs)
        }
    }
}
