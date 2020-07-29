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
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use futures::future::{BoxFuture, FutureExt};
use thiserror::Error;

use crate::{
    git::{
        self,
        p2p::server::GitServer,
        storage::{self, Storage as GitStorage},
    },
    internal::{borrow::TryToOwned, channel::Fanout},
    keys::{self, AsPKCS8},
    net::{
        connection::LocalInfo,
        discovery::Discovery,
        gossip::{self, LocalStorage, PutResult},
        protocol::Protocol,
        quic::{self, Endpoint},
    },
    paths::Paths,
    peer::{Originates, OriginatesRef, PeerId},
    signer::Signer,
    uri::{self, RadUrl, RadUrn},
};

pub mod types;
pub use types::*;

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum GitFetchError {
    #[error("Already have {0}")]
    KnownObject(git2::Oid),

    #[error(transparent)]
    Store(#[from] git::storage::Error),
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BootstrapError {
    #[error("Failed to bind to {addr}")]
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
    pub result: PutResult,
}

#[derive(Clone)]
pub struct PeerConfig<Disco, Signer> {
    pub signer: Signer,
    pub paths: Paths,
    pub listen_addr: SocketAddr,
    pub gossip_params: gossip::MembershipParams,
    pub disco: Disco,
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

/// Main entry point for `radicle-link` applications on top of a connected
/// [`Peer`]
///
/// Note that a [`PeerApi`] is neither [`Clone`] nor [`Sync`], because it owns
/// an open handle to the backend git storage (which requires external
/// synchronisation and refcounting if applicable). It is nevertheless possible
/// to obtain an owned copy by calling `try_to_owned`, which will re-open the
/// git storage. The tradeoffs are that a. concurrent modifications of the
/// storage may not always be consistent between two instances, and b. that the
/// `try_to_owned` operation is fallible due to having to perform IO. Also note
/// that the `TryToOwned` trait is not currently considered a stable API.
pub struct PeerApi<S> {
    protocol: Protocol<PeerStorage<S>, Gossip>,
    storage: GitStorage<S>,
    subscribers: Fanout<PeerEvent>,
    paths: Paths,
}

impl<S: Clone> PeerApi<S> {
    pub fn protocol(&self) -> &Protocol<PeerStorage<S>, Gossip> {
        &self.protocol
    }

    pub fn storage(&self) -> &GitStorage<S> {
        &self.storage
    }

    pub fn peer_id(&self) -> &PeerId {
        self.storage.peer_id()
    }

    pub fn subscribe(&self) -> impl Future<Output = impl futures::Stream<Item = PeerEvent>> {
        // Nb. `PeerApi` is not `Sync`, which means that any `async` method we'd
        // define on it can never be `await`ed.
        let subscribers = self.subscribers.clone();
        async move { subscribers.subscribe().await }
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }
}

impl<S: Clone> TryToOwned for PeerApi<S> {
    type Owned = Self;
    type Error = ApiError;

    fn try_to_owned(&self) -> Result<Self::Owned, Self::Error> {
        let storage = self.storage.try_to_owned()?;
        Ok(Self {
            protocol: self.protocol.clone(),
            storage,
            subscribers: self.subscribers.clone(),
            paths: self.paths.clone(),
        })
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

    storage: GitStorage<S>,

    protocol: Protocol<PeerStorage<S>, Gossip>,
    run_loop: RunLoop,

    subscribers: Fanout<PeerEvent>,
}

impl<S> Peer<S>
where
    S: Signer + Clone,
    S::Error: keys::SignError,
{
    pub fn listen_addr(&self) -> SocketAddr {
        self.listen_addr
    }

    pub fn peer_id(&self) -> &PeerId {
        self.storage.peer_id()
    }

    pub fn accept(self) -> Result<(PeerApi<S>, RunLoop), AcceptError> {
        let api = PeerApi {
            storage: self.storage,
            protocol: self.protocol,
            subscribers: self.subscribers,
            paths: self.paths,
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
        let storage = GitStorage::open_or_init(&config.paths, config.signer.clone())?;
        let peer_storage = {
            let storage = storage.reopen()?;
            PeerStorage {
                inner: Arc::new(Mutex::new(storage)),
                subscribers: subscribers.clone(),
            }
        };

        let gossip = gossip::Protocol::new(
            &peer_id,
            gossip::PeerAdvertisement::new(listen_addr.ip(), listen_addr.port()),
            config.gossip_params,
            peer_storage,
        );

        let protocol = Protocol::new(gossip, git);
        git::p2p::transport::register()
            .register_stream_factory(&peer_id, Box::new(protocol.clone()));

        let run_loop = protocol
            .clone()
            .run(endpoint, config.disco.discover())
            .boxed();

        Ok(Self {
            paths: config.paths,
            listen_addr,
            storage,
            protocol,
            run_loop,
            subscribers,
        })
    }
}

#[derive(Clone)]
pub struct PeerStorage<S> {
    inner: Arc<Mutex<GitStorage<S>>>,
    subscribers: Fanout<PeerEvent>,
}

impl<S> PeerStorage<S>
where
    S: Signer + Clone,
    S::Error: keys::SignError,
{
    fn git_fetch<'a>(
        &'a self,
        from: &PeerId,
        urn: impl Into<OriginatesRef<'a, RadUrn>>,
        head: impl Into<Option<git2::Oid>>,
    ) -> Result<(), GitFetchError> {
        let git = self.inner.lock().unwrap();
        let urn = urn_context(git.peer_id(), urn);

        if let Some(head) = head.into() {
            if git.has_commit(&urn, head)? {
                return Err(GitFetchError::KnownObject(head));
            }
        }

        let url = RadUrl {
            authority: from.clone(),
            urn,
        };
        git.fetch_repo(url).map_err(|e| e.into())
    }

    /// Determine if we have the given object locally
    fn git_has<'a>(
        &'a self,
        urn: impl Into<OriginatesRef<'a, RadUrn>>,
        head: impl Into<Option<git2::Oid>>,
    ) -> bool {
        let git = self.inner.lock().unwrap();
        let urn = urn_context(git.peer_id(), urn);
        match head.into() {
            None => git.has_urn(&urn).unwrap_or(false),
            Some(head) => git.has_commit(&urn, head).unwrap_or(false),
        }
    }
}

/// If applicable, map the [`uri::Path`] of the given [`RadUrn`] to
/// `refs/remotes/<origin>/<path>`
fn urn_context<'a>(local_peer_id: &PeerId, urn: impl Into<OriginatesRef<'a, RadUrn>>) -> RadUrn {
    let OriginatesRef { from, value } = urn.into();
    let urn = value.clone();

    if from == local_peer_id {
        return urn;
    }

    let path = urn
        .path
        .strip_prefix("refs/")
        .map(|tail| {
            uri::Path::parse(tail).expect("`Path` is still valid after stripping a valid prefix")
        })
        .unwrap_or(urn.path);

    let mut remote = uri::Path::parse(format!("refs/remotes/{}", from)).expect("Known valid path");
    remote.push(path);

    RadUrn {
        path: remote,
        ..urn
    }
}

#[async_trait]
impl<S> LocalStorage for PeerStorage<S>
where
    S: Signer + Clone,
    S::Error: keys::SignError,
{
    type Update = Gossip;

    async fn put(&self, provider: &PeerId, has: Self::Update) -> PutResult {
        let span = tracing::info_span!("Peer::LocalStorage::put");
        let _guard = span.enter();

        match has.urn.proto {
            uri::Protocol::Git => {
                let res = match has.rev {
                    // TODO: may need to fetch eagerly if we tracked while offline (#141)
                    None => PutResult::Uninteresting,
                    Some(Rev::Git(head)) => {
                        let res = {
                            let this = self.clone();
                            let provider = provider.clone();
                            let has = has.clone();
                            tokio::task::spawn_blocking(move || {
                                this.git_fetch(
                                    &provider,
                                    OriginatesRef {
                                        from: &has.origin,
                                        value: &has.urn,
                                    },
                                    head,
                                )
                            })
                            .await
                            .unwrap()
                        };

                        match res {
                            Ok(()) => {
                                if !self.ask(has.clone()).await {
                                    tracing::warn!(
                                        provider = %provider,
                                        has.origin = %has.origin,
                                        has.urn = %has.urn,
                                        "Provider announced non-existent rev"
                                    );
                                    PutResult::Stale
                                } else {
                                    PutResult::Applied
                                }
                            },
                            Err(e) => match e {
                                GitFetchError::KnownObject(_) => PutResult::Stale,
                                GitFetchError::Store(storage::Error::NoSuchUrn(_)) => {
                                    PutResult::Uninteresting
                                },
                                e => {
                                    tracing::error!(err = %e, "Fetch error");
                                    PutResult::Error
                                },
                            },
                        }
                    },
                };

                self.subscribers
                    .emit(PeerEvent::GossipFetch(FetchInfo {
                        provider: provider.clone(),
                        gossip: has,
                        result: res,
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
            uri::Protocol::Git => self.git_has(
                &Originates {
                    from: want.origin,
                    value: want.urn,
                },
                want.rev.map(|Rev::Git(head)| head),
            ),
        }
    }
}
