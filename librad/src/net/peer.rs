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
    sync::Arc,
    time::Duration,
};

use either::Either::{self, Left, Right};
use futures::{
    future::{self, BoxFuture, FutureExt},
    stream::StreamExt,
};
use futures_timer::Delay;
use git_ext::{self as ext, reference};
use thiserror::Error;
use tokio::task::spawn_blocking;

use crate::{
    git::{
        self,
        p2p::{server::GitServer, transport::GitStreamFactory},
        replication,
        storage,
        tracking,
    },
    identities::{git::Urn, urn},
    internal::channel::Fanout,
    keys::AsPKCS8,
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
    Tracking(#[from] tracking::Error),

    #[error(transparent)]
    Replication(#[from] replication::Error),

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
    #[tracing::instrument(skip(self))]
    pub async fn providers(
        &self,
        urn: Urn,
        timeout: Duration,
    ) -> impl futures::Stream<Item = PeerInfo<IpAddr>> {
        let protocol = self.protocol.clone();

        let urn2 = urn.clone();
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
                        ProtocolEvent::Gossip(gossip::Info::Has(gossip::Has { provider, val }))
                            if val.urn == urn2 =>
                        {
                            Some(provider)
                        },
                        _ => None,
                    })
                })
                .map(Ok),
        )
        .take_while(|x| future::ready(x.is_ok()))
        .map(Result::unwrap);

        protocol
            .query(Gossip {
                urn,
                rev: None,
                origin: None,
            })
            .await;

        providers
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
{
    async fn git_fetch(
        &self,
        from: PeerId,
        urn: Either<Urn, Originates<Urn>>,
        head: impl Into<Option<git2::Oid>>,
    ) -> Result<(), PeerStorageError> {
        let git = self.inner.get().await?;
        let urn = urn_context(*git.peer_id(), urn);
        let head = head.into().map(ext::Oid::from);

        spawn_blocking(move || {
            if let Some(head) = head {
                if git.has_commit(&urn, head)? {
                    return Err(PeerStorageError::KnownObject(*head));
                }
            }

            Ok(replication::replicate(&git, None, urn, from, None)?)
        })
        .await
        .expect("`PeerStorage::git_fetch` panicked")
    }

    /// Determine if we have the given object locally
    async fn git_has(
        &self,
        urn: Either<Urn, Originates<Urn>>,
        head: impl Into<Option<git2::Oid>>,
    ) -> bool {
        let git = self.inner.get().await.unwrap();
        let urn = urn_context(*git.peer_id(), urn);
        let head = head.into().map(ext::Oid::from);
        spawn_blocking(move || match head {
            None => git.has_urn(&urn).unwrap_or(false),
            Some(head) => git.has_commit(&urn, head).unwrap_or(false),
        })
        .await
        .expect("`PeerStorage::git_has` panicked")
    }

    async fn is_tracked(&self, urn: Urn, peer: PeerId) -> Result<bool, PeerStorageError> {
        let git = self.inner.get().await?;
        Ok(
            spawn_blocking(move || tracking::is_tracked(&git, &urn, peer))
                .await
                .expect("`PeerStorage::is_tracked` panicked")?,
        )
    }
}

/// If applicable, map the `path` of the given [`Urn`] to
/// `refs/remotes/<origin>/<path>`
fn urn_context(local_peer_id: PeerId, urn: Either<Urn, Originates<Urn>>) -> Urn {
    fn remote(urn: Urn, peer: PeerId) -> Urn {
        let path = reflike!("refs/remotes").join(peer).join(
            ext::RefLike::from(reference::Qualified::from(
                urn.path.unwrap_or_else(|| urn::DEFAULT_PATH.clone()),
            ))
            .strip_prefix("refs")
            .unwrap(),
        );

        Urn {
            id: urn.id,
            path: Some(path),
        }
    }

    fn local(urn: Urn) -> Urn {
        urn.map_path(|path| {
            path.or_else(|| Some(urn::DEFAULT_PATH.clone()))
                .map(reference::Qualified::from)
                .map(ext::RefLike::from)
        })
    }

    match urn {
        Left(urn) => local(urn),
        Right(Originates { from, value: urn }) if from == local_peer_id => local(urn),
        Right(Originates { from, value: urn }) => remote(urn, from),
    }
}

#[async_trait]
impl<S> LocalStorage for PeerStorage<S>
where
    S: Signer + Clone,
{
    type Update = Gossip;

    #[tracing::instrument(skip(self))]
    async fn put(&self, provider: PeerId, has: Self::Update) -> PutResult<Self::Update> {
        let peer_id = has.origin.unwrap_or(provider);
        let is_tracked = match self.is_tracked(has.urn.clone(), peer_id).await {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(err = %e, "error determining tracking status");
                return PutResult::Error;
            },
        };

        let res = match has.rev {
            // TODO: may need to fetch eagerly if we tracked while offline (#141)
            Some(Rev::Git(head)) if is_tracked => {
                let urn = match has.origin {
                    Some(origin) => Right(Originates {
                        from: origin,
                        value: has.urn.clone(),
                    }),
                    None => Left(has.urn.clone()),
                };

                let this = self.clone();
                let res = this.git_fetch(provider, urn.clone(), head).await;

                match res {
                    Ok(()) => {
                        if this.git_has(urn, head).await {
                            PutResult::Applied(Gossip {
                                origin: Some(peer_id),
                                ..has.clone()
                            })
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
    }

    #[tracing::instrument(level = "debug", skip(self))]
    async fn ask(&self, want: Self::Update) -> bool {
        self.git_has(
            match want.origin {
                Some(origin) => Right(Originates {
                    from: origin,
                    value: want.urn,
                }),
                None => Left(want.urn),
            },
            want.rev.map(|Rev::Git(head)| head),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod urn_context {
        use super::*;
        use crate::keys::SecretKey;

        lazy_static! {
            static ref LOCAL_PEER_ID: PeerId = PeerId::from(SecretKey::from_seed([
                188, 124, 109, 100, 178, 93, 115, 53, 15, 22, 114, 181, 15, 211, 233, 104, 32, 189,
                9, 162, 235, 148, 204, 172, 21, 117, 34, 9, 236, 247, 238, 113
            ]));
            static ref OTHER_PEER_ID: PeerId = PeerId::from(SecretKey::from_seed([
                236, 225, 197, 234, 16, 153, 83, 54, 15, 203, 86, 253, 157, 81, 144, 96, 106, 99,
                65, 129, 8, 181, 125, 141, 120, 122, 58, 48, 22, 97, 32, 9
            ]));
            static ref ZERO_OID: ext::Oid = git2::Oid::zero().into();
        }

        #[test]
        fn direct_empty() {
            let urn = Urn::new(*ZERO_OID);
            let ctx = urn_context(*LOCAL_PEER_ID, Left(urn.clone()));
            assert_eq!(urn.with_path(urn::DEFAULT_PATH.clone()), ctx)
        }

        #[test]
        fn direct_onelevel() {
            let urn = Urn::new(*ZERO_OID).with_path(reflike!("ban/ana"));
            let ctx = urn_context(*LOCAL_PEER_ID, Left(urn.clone()));
            assert_eq!(urn.with_path(reflike!("refs/heads/ban/ana")), ctx)
        }

        #[test]
        fn direct_qualified() {
            let urn = Urn::new(*ZERO_OID).with_path(reflike!("refs/heads/next"));
            let ctx = urn_context(*LOCAL_PEER_ID, Left(urn.clone()));
            assert_eq!(urn, ctx)
        }

        #[test]
        fn remote_empty() {
            let urn = Urn::new(*ZERO_OID);
            let ctx = urn_context(
                *LOCAL_PEER_ID,
                Right(Originates {
                    from: *OTHER_PEER_ID,
                    value: urn.clone(),
                }),
            );
            assert_eq!(
                urn.with_path(
                    reflike!("refs/remotes")
                        .join(*OTHER_PEER_ID)
                        .join(urn::DEFAULT_PATH.strip_prefix("refs").unwrap())
                ),
                ctx
            )
        }

        #[test]
        fn remote_onelevel() {
            let urn = Urn::new(*ZERO_OID).with_path(reflike!("ban/ana"));
            let ctx = urn_context(
                *LOCAL_PEER_ID,
                Right(Originates {
                    from: *OTHER_PEER_ID,
                    value: urn.clone(),
                }),
            );
            assert_eq!(
                urn.with_path(
                    reflike!("refs/remotes")
                        .join(*OTHER_PEER_ID)
                        .join(reflike!("heads/ban/ana"))
                ),
                ctx
            )
        }

        #[test]
        fn remote_qualified() {
            let urn = Urn::new(*ZERO_OID).with_path(reflike!("refs/heads/next"));
            let ctx = urn_context(
                *LOCAL_PEER_ID,
                Right(Originates {
                    from: *OTHER_PEER_ID,
                    value: urn.clone(),
                }),
            );
            assert_eq!(
                urn.with_path(
                    reflike!("refs/remotes")
                        .join(*OTHER_PEER_ID)
                        .join(reflike!("heads/next"))
                ),
                ctx
            )
        }

        #[test]
        fn self_origin_empty() {
            let urn = Urn::new(*ZERO_OID);
            let ctx = urn_context(
                *LOCAL_PEER_ID,
                Right(Originates {
                    from: *LOCAL_PEER_ID,
                    value: urn.clone(),
                }),
            );
            assert_eq!(urn.with_path(urn::DEFAULT_PATH.clone()), ctx)
        }

        #[test]
        fn self_origin_onelevel() {
            let urn = Urn::new(*ZERO_OID).with_path(reflike!("ban/ana"));
            let ctx = urn_context(
                *LOCAL_PEER_ID,
                Right(Originates {
                    from: *LOCAL_PEER_ID,
                    value: urn.clone(),
                }),
            );
            assert_eq!(urn.with_path(reflike!("refs/heads/ban/ana")), ctx)
        }

        #[test]
        fn self_origin_qualified() {
            let urn = Urn::new(*ZERO_OID).with_path(reflike!("refs/heads/next"));
            let ctx = urn_context(
                *LOCAL_PEER_ID,
                Right(Originates {
                    from: *LOCAL_PEER_ID,
                    value: urn.clone(),
                }),
            );
            assert_eq!(urn, ctx)
        }
    }
}
