// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use std::{
    future::Future,
    io,
    net::{IpAddr, SocketAddr},
    ops::Deref,
    sync::Arc,
    time::Duration,
};

use either::Either;
use futures::{
    future::{self, BoxFuture, FutureExt},
    stream::StreamExt,
};
use futures_timer::Delay;
use thiserror::Error;
use tokio::task::spawn_blocking;
use tracing_futures::Instrument as _;

use crate::{
    git::{
        self,
        p2p::{server::GitServer, transport::GitStreamFactory},
        storage,
    },
    internal::channel::Fanout,
    keys::{self, AsPKCS8},
    net::{
        connection::LocalInfo,
        discovery::Discovery,
        gossip::{self, LocalStorage, PeerInfo, PutResult},
        protocol::{Protocol, ProtocolEvent},
        quic::{self, Endpoint},
    },
    paths::Paths,
    peer::{Originates, PeerId},
    signer::Signer,
    uri::{self, RadUrl, RadUrn},
};

pub mod types;
pub use types::*;

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PeerStorageError {
    #[error("already have {0}")]
    KnownObject(git2::Oid),

    #[error(transparent)]
    Store(#[from] git::storage::Error),

    #[error(transparent)]
    Pool(#[from] deadpool::managed::PoolError<git::storage::Error>),
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BootstrapError {
    #[error("failed to bind to {addr}")]
    Bind {
        addr: SocketAddr,
        source: quic::Error,
    },

    #[error(transparent)]
    Storage(#[from] git::storage::Error),

    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AcceptError {
    #[error(transparent)]
    Storage(#[from] git::storage::Error),
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ApiError {
    #[error(transparent)]
    Storage(#[from] git::storage::Error),

    #[error(transparent)]
    Pool(#[from] deadpool::managed::PoolError<git::storage::Error>),
}

/// Upstream events.
///
/// A [`Peer`] exhibits "background" behaviour as it reacts to gossip. This
/// behaviour can be observed by using [`Peer::subscribe`].
#[derive(Clone, Debug)]
pub enum PeerEvent {
    GossipFetch(FetchInfo),
}

/// Event payload for a fetch triggered by [`LocalStorage::put`]
#[derive(Clone, Debug)]
pub struct FetchInfo {
    pub provider: PeerId,
    pub gossip: Gossip,
    pub result: PutResult<Gossip>,
}

#[derive(Clone)]
pub struct PeerConfig<Disco, Signer> {
    pub signer: Signer,
    pub paths: Paths,
    pub listen_addr: SocketAddr,
    pub gossip_params: gossip::MembershipParams,
    pub disco: Disco,
    pub storage_config: StorageConfig,
}

impl<D, S> PeerConfig<D, S>
where
    S: Signer + Clone + AsPKCS8,
    S::Error: keys::SignError,
    D: Discovery<Addr = SocketAddr>,
    <D as Discovery>::Stream: 'static,
{
    pub async fn try_into_peer(self) -> Result<Peer<S>, BootstrapError> {
        Peer::bootstrap(self).await
    }
}

#[derive(Clone, Copy)]
pub struct StorageConfig {
    /// Number of [`Storage`] instances to pool for [`PeerApi`] consumers.
    ///
    /// Default: the number of physical cores available
    pub user_pool_size: usize,

    /// Number of [`Storage`] instances to reserve for protocol use.
    ///
    /// Default: the number of physical cores available
    pub protocol_pool_size: usize,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            user_pool_size: num_cpus::get_physical(),
            protocol_pool_size: num_cpus::get_physical(),
        }
    }
}

/// Main entry point for `radicle-link` applications on top of a connected
/// [`Peer`]
#[derive(Clone)]
pub struct PeerApi<S> {
    listen_addr: SocketAddr,
    peer_id: PeerId,
    protocol: Protocol<PeerStorage<S>, Gossip>,
    storage: storage::Pool<S>,
    subscribers: Fanout<PeerEvent>,
    paths: Paths,

    _git_transport_protocol_ref: Arc<Box<dyn GitStreamFactory>>,
}

impl<S> PeerApi<S>
where
    S: Signer + Clone,
    S::Error: keys::SignError,
{
    pub fn listen_addr(&self) -> SocketAddr {
        self.listen_addr
    }

    pub fn protocol(&self) -> &Protocol<PeerStorage<S>, Gossip> {
        &self.protocol
    }

    pub async fn with_storage<F, A>(&self, blocking: F) -> Result<A, ApiError>
    where
        F: FnOnce(&storage::Storage<S>) -> A + Send + 'static,
        A: Send + 'static,
    {
        let storage = self.storage.get().await?;
        Ok(spawn_blocking(move || blocking(&storage))
            .await
            .expect("blocking operation on storage panicked"))
    }

    pub fn peer_id(&self) -> PeerId {
        self.peer_id
    }

    pub fn subscribe(&self) -> impl Future<Output = impl futures::Stream<Item = PeerEvent>> {
        // Nb. `PeerApi` is not `Sync`, which means that any `async` method we'd
        // define on it can never be `await`ed.
        let subscribers = self.subscribers.clone();
        async move { subscribers.subscribe().await }
    }

    /// Query the network for providers of the given [`RadUrn`].
    ///
    /// This is a convenience for the special case of issuing a gossip `Want`
    /// message where we don't know a specific revision, nor an origin peer.
    /// Consequently, any `Have` message with a matching `urn` should do for
    /// attempting a clone, even if it isn't a direct response to our query.
    ///
    /// Note that there is no guarantee that a peer who claims to provide the
    /// [`RadUrn`] actually has it, nor that it is reachable using any of
    /// the addresses contained in [`PeerInfo`]. The implementation may
    /// change in the future to answer the query from a local cache first.
    ///
    /// The returned [`futures::Stream`] will be complete after the supplied
    /// `timeout` has elapsed, whether or not any responses have been yielded
    /// thus far. This is to prevent callers from polling the stream
    /// indefinitely, even though no more responses can be expected. A realistic
    /// timeout value is in the order of 10s of seconds.
    pub fn providers(
        &self,
        urn: RadUrn,
        timeout: Duration,
    ) -> impl Future<Output = impl futures::Stream<Item = PeerInfo<IpAddr>>> {
        let span = tracing::trace_span!("PeerApi::providers", urn = %urn);
        let protocol = self.protocol.clone();
        let target_urn = urn.clone();

        async move {
            let providers = futures::stream::select(
                futures::stream::once(
                    async move {
                        Delay::new(timeout).await;
                        Err("timed out")
                    }
                    .boxed(),
                ),
                protocol
                    .subscribe()
                    .await
                    .filter_map(move |evt| {
                        future::ready(match evt {
                            ProtocolEvent::Gossip(gossip::Info::Has(gossip::Has {
                                provider,
                                val,
                            })) if val.urn == urn => Some(provider),
                            _ => None,
                        })
                    })
                    .map(Ok),
            )
            .take_while(|x| future::ready(x.is_ok()))
            .map(Result::unwrap);

            protocol
                .query(Gossip {
                    urn: target_urn,
                    rev: None,
                    origin: None,
                })
                .instrument(span)
                .await;

            providers
        }
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }
}

/// Future driving the networking stack
pub type RunLoop = BoxFuture<'static, ()>;

/// A bootstrapped network peer
///
/// The peer is already bound to a network socket, and ready to execute the
/// protocol stack. In order to actually send and receive from the network, the
/// [`Peer`] needs to be exchanged for a [`Future`] using `accept`, which must
/// be polled to make progress. `accept` also returns a [`PeerApi`], which
/// provides methods for passing messages up- and downstream.
///
/// The intermediate, bound state is mainly useful to query the [`SocketAddr`]
/// chosen by the operating system when the [`Peer`] was bootstrapped using
/// `0.0.0.0:0`.
pub struct Peer<S> {
    paths: Paths,
    listen_addr: SocketAddr,
    peer_id: PeerId,

    storage: storage::Pool<S>,
    protocol: Protocol<PeerStorage<S>, Gossip>,
    run_loop: RunLoop,
    subscribers: Fanout<PeerEvent>,

    // We cannot cast `Arc<Box<Protocol<A, B>>>` to `Arc<Box<dyn GitStreamFactory>>`
    // apparenty, so need to keep an `Arc` of the trait object here in order to
    // hand out `Weak` pointers to
    // `git::p2p::transport::RadTransport::register_stream_factory`
    _git_transport_protocol_ref: Arc<Box<dyn GitStreamFactory>>,
}

impl<S> Peer<S>
where
    S: Signer + Clone,
    S::Error: keys::SignError,
{
    pub fn listen_addr(&self) -> SocketAddr {
        self.listen_addr
    }

    pub fn peer_id(&self) -> PeerId {
        self.peer_id
    }

    pub fn accept(self) -> Result<(PeerApi<S>, RunLoop), AcceptError> {
        let api = PeerApi {
            listen_addr: self.listen_addr,
            peer_id: self.peer_id,
            storage: self.storage,
            protocol: self.protocol,
            subscribers: self.subscribers,
            paths: self.paths,

            _git_transport_protocol_ref: self._git_transport_protocol_ref,
        };
        Ok((api, self.run_loop))
    }

    async fn bootstrap<D>(config: PeerConfig<D, S>) -> Result<Self, BootstrapError>
    where
        S: AsPKCS8 + 'static,
        D: Discovery<Addr = SocketAddr>,
        <D as Discovery>::Stream: 'static,
    {
        let peer_id = PeerId::from_signer(&config.signer);

        let git = GitServer::new(&config.paths);

        let endpoint = Endpoint::bind(&config.signer, config.listen_addr)
            .await
            .map_err(|e| BootstrapError::Bind {
                addr: config.listen_addr,
                source: e,
            })?;
        let listen_addr = endpoint.local_addr()?;

        let subscribers = Fanout::new();
        let user_storage = storage::Pool::new(
            storage::pool::Config::new(config.paths.clone(), config.signer.clone()),
            config.storage_config.user_pool_size,
        );
        let peer_storage = PeerStorage {
            inner: storage::Pool::new(
                storage::pool::Config::new(config.paths.clone(), config.signer),
                config.storage_config.protocol_pool_size,
            ),
            subscribers: subscribers.clone(),
        };

        let gossip = gossip::Protocol::new(
            peer_id,
            gossip::PeerAdvertisement::new(listen_addr.ip(), listen_addr.port()),
            config.gossip_params,
            peer_storage,
        );

        let (protocol, run_loop) = Protocol::new(gossip, git, endpoint, config.disco.discover());
        let _git_transport_protocol_ref =
            Arc::new(Box::new(protocol.clone()) as Box<dyn GitStreamFactory>);
        git::p2p::transport::register()
            .register_stream_factory(peer_id, Arc::downgrade(&_git_transport_protocol_ref));

        Ok(Self {
            paths: config.paths,
            listen_addr,
            peer_id,
            storage: user_storage,
            protocol,
            run_loop,
            subscribers,
            _git_transport_protocol_ref,
        })
    }
}

#[derive(Clone)]
pub struct PeerStorage<S> {
    inner: storage::Pool<S>,
    subscribers: Fanout<PeerEvent>,
}

impl<S> PeerStorage<S>
where
    S: Signer + Clone,
    S::Error: keys::SignError,
{
    async fn git_fetch<'a>(
        &'a self,
        from: PeerId,
        urn: Either<RadUrn, Originates<RadUrn>>,
        head: impl Into<Option<git2::Oid>>,
    ) -> Result<(), PeerStorageError> {
        let git = self.inner.get().await?;
        let urn = urn_context(&git.peer_id(), urn);
        let head = head.into();

        spawn_blocking(move || {
            if let Some(head) = head {
                if git.has_commit(&urn, head)? {
                    return Err(PeerStorageError::KnownObject(head));
                }
            }

            let url = RadUrl {
                authority: from,
                urn,
            };
            git.fetch_repo(url, None).map_err(|e| e.into())
        })
        .await
        .expect("`PeerStorage::git_fetch` panicked")
    }

    /// Determine if we have the given object locally
    async fn git_has(
        &self,
        urn: Either<RadUrn, Originates<RadUrn>>,
        head: impl Into<Option<git2::Oid>>,
    ) -> bool {
        let git = self.inner.get().await.unwrap();
        let urn = urn_context(&git.peer_id(), urn);
        let head = head.into();
        spawn_blocking(move || match head {
            None => git.has_urn(&urn).unwrap_or(false),
            Some(head) => git.has_commit(&urn, head).unwrap_or(false),
        })
        .await
        .expect("`PeerStorage::git_has` panicked")
    }

    async fn is_tracked(&self, urn: RadUrn, peer: PeerId) -> Result<bool, PeerStorageError> {
        let git = self.inner.get().await?;
        Ok(spawn_blocking(move || git.is_tracked(&urn, &peer))
            .await
            .expect("`PeerStorage::is_tracked` panicked")?)
    }
}

/// If applicable, map the [`uri::Path`] of the given [`RadUrn`] to
/// `refs/remotes/<origin>/<path>`
fn urn_context(local_peer_id: &PeerId, urn: Either<RadUrn, Originates<RadUrn>>) -> RadUrn {
    match urn {
        Either::Left(urn) => RadUrn {
            path: qualify_path(&urn.path),
            ..urn
        },
        Either::Right(Originates { from, value }) => {
            let urn = value;

            if from == *local_peer_id {
                return urn;
            }

            let path = qualify_path(&urn.path);
            let mut remote =
                uri::Path::parse(format!("refs/remotes/{}", from)).expect("Known valid path");
            remote.push(path);

            RadUrn {
                path: remote,
                ..urn
            }
        },
    }
}

/// Strip the `refs/` prefix of the provided path.
///
/// If there's no 'refs/' prefix we assume we need to add 'heads/' to the
/// reference name.
fn qualify_path(path: &uri::Path) -> uri::Path {
    if path.is_empty() {
        return uri::Path::empty();
    }

    match path.strip_prefix("refs/") {
        Some(tail) => {
            uri::Path::parse(tail).expect("`Path` is still valid after stripping a valid prefix")
        },
        None => uri::Path::parse(format!("heads/{}", path.deref()))
            .expect("`Path` is valid after adding 'heads/` prefix"),
    }
}

#[async_trait]
impl<S> LocalStorage for PeerStorage<S>
where
    S: Signer + Clone,
    S::Error: keys::SignError,
{
    type Update = Gossip;

    async fn put(&self, provider: PeerId, mut has: Self::Update) -> PutResult<Self::Update> {
        let span = tracing::info_span!("Peer::LocalStorage::put");
        let _guard = span.enter();

        match has.urn.proto {
            uri::Protocol::Git => {
                let urn = match has.origin.as_ref() {
                    None => Either::Left(has.urn.clone()),
                    Some(origin) => Either::Right(Originates {
                        from: *origin,
                        value: has.urn.clone(),
                    }),
                };

                // Set the origin to which remote's changes we end up fetching.
                let peer_id = has.origin.unwrap_or_else(|| provider);
                has.origin = Some(peer_id);

                let is_tracked = match self.is_tracked(has.urn.clone(), peer_id).await {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::error!(err = %e, "Git::Storage::is_tracked error");
                        return PutResult::Error;
                    },
                };
                let res = match has.rev {
                    // TODO: may need to fetch eagerly if we tracked while offline (#141)
                    Some(Rev::Git(head)) if is_tracked => {
                        let res = {
                            let this = self.clone();
                            this.git_fetch(provider, urn, head).await
                        };

                        match res {
                            Ok(()) => {
                                if self.ask(has.clone()).await {
                                    PutResult::Applied(has.clone())
                                } else {
                                    tracing::warn!(
                                        provider = %provider,
                                        has.origin = ?has.origin,
                                        has.urn = %has.urn,
                                        "Provider announced non-existent rev"
                                    );
                                    PutResult::Stale
                                }
                            },
                            Err(e) => match e {
                                PeerStorageError::KnownObject(_) => PutResult::Stale,
                                PeerStorageError::Store(storage::Error::NoSuchUrn(_)) => {
                                    PutResult::Uninteresting
                                },
                                e => {
                                    tracing::error!(err = %e, "Fetch error");
                                    PutResult::Error
                                },
                            },
                        }
                    },
                    // The update is uninteresting if it refers to no revision
                    // or if its originated by a peer we are not tracking.
                    _ => PutResult::Uninteresting,
                };

                self.subscribers
                    .emit(PeerEvent::GossipFetch(FetchInfo {
                        provider,
                        gossip: has,
                        result: res.clone(),
                    }))
                    .await;

                res
            },
        }
    }

    async fn ask(&self, want: Self::Update) -> bool {
        let span = tracing::info_span!("Peer::LocalStorage::ask");
        let _guard = span.enter();

        match want.urn.proto {
            uri::Protocol::Git => {
                self.git_has(
                    match want.origin {
                        Some(origin) => Either::Right(Originates {
                            from: origin,
                            value: want.urn,
                        }),
                        None => Either::Left(want.urn),
                    },
                    want.rev.map(|Rev::Git(head)| head),
                )
                .await
            },
        }
    }
}
