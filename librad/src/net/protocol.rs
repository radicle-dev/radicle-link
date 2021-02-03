// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    fmt::Debug,
    future::Future,
    net::SocketAddr,
    ops::Deref,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures::{
    channel::mpsc,
    future::{self, BoxFuture, FutureExt as _, TryFutureExt as _},
    stream::{BoxStream, StreamExt as _},
};
use governor::{Quota, RateLimiter};
use nonzero_ext::nonzero;
use parking_lot::{Mutex, RwLock};
use rand_pcg::Pcg64Mcg;
use tokio::sync::broadcast as tincan;
use tracing::Instrument as _;

use super::{
    connection::{LocalAddr, LocalPeer},
    quic,
    upgrade,
    Network,
};
use crate::{
    git::{
        self,
        p2p::{
            server::GitServer,
            transport::{GitStream, GitStreamFactory},
        },
        replication,
        storage::{self, PoolError, PooledRef},
    },
    paths::Paths,
    signer::Signer,
    PeerId,
};

pub use tokio::sync::broadcast::RecvError;

pub mod broadcast;
pub mod error;
pub mod event;
pub mod gossip;
pub mod graft;
pub mod membership;

mod info;
pub use info::{Capability, PartialPeerInfo, PeerAdvertisement, PeerInfo};

mod accept;
mod control;
mod io;
mod tick;

#[derive(Clone, Debug)]
pub struct Config {
    pub paths: Paths,
    pub listen_addr: SocketAddr,
    pub membership: membership::Params,
    pub network: Network,
    pub replication: replication::Config,
    pub graft: graft::Config,
    // TODO: transport, ...
}

pub struct Bound<S> {
    phone: TinCans,
    state: State<S>,
    incoming: BoxStream<'static, quic::Result<(quic::Connection, quic::IncomingStreams<'static>)>>,
    periodic: mpsc::Receiver<membership::Periodic<SocketAddr>>,
}

impl<S> Bound<S> {
    pub fn peer_id(&self) -> PeerId {
        self.state.local_id
    }

    pub fn listen_addrs(&self) -> std::io::Result<Vec<SocketAddr>> {
        self.state.endpoint.listen_addrs()
    }

    pub async fn accept<D>(self, disco: D) -> Result<!, quic::Error>
    where
        S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
        D: futures::Stream<Item = (PeerId, Vec<SocketAddr>)> + Send + 'static,
    {
        accept(self, disco).await
    }
}

impl<S> LocalPeer for Bound<S> {
    fn local_peer_id(&self) -> PeerId {
        self.peer_id()
    }
}

impl<S> LocalAddr for Bound<S> {
    type Addr = SocketAddr;

    fn listen_addrs(&self) -> std::io::Result<Vec<Self::Addr>> {
        self.state.endpoint.listen_addrs()
    }
}

#[derive(Clone)]
pub struct TinCans {
    downstream: tincan::Sender<event::Downstream>,
    upstream: tincan::Sender<event::Upstream>,
}

impl TinCans {
    pub fn new() -> Self {
        Self {
            downstream: tincan::channel(16).0,
            upstream: tincan::channel(16).0,
        }
    }

    pub fn announce(&self, have: gossip::Payload) -> Result<(), gossip::Payload> {
        use event::{downstream::Gossip::Announce, Downstream};

        self.downstream
            .send(Downstream::Gossip(Announce(have)))
            .and(Ok(()))
            .map_err(|tincan::SendError(e)| match e {
                Downstream::Gossip(g) => g.payload(),
                _ => unreachable!(),
            })
    }

    pub fn query(&self, want: gossip::Payload) -> Result<(), gossip::Payload> {
        use event::{downstream::Gossip::Query, Downstream};

        self.downstream
            .send(Downstream::Gossip(Query(want)))
            .and(Ok(()))
            .map_err(|tincan::SendError(e)| match e {
                Downstream::Gossip(g) => g.payload(),
                _ => unreachable!(),
            })
    }

    pub async fn connected_peers(&self) -> Vec<PeerId> {
        use event::{downstream::Info::*, Downstream};

        let (tx, rx) = tokio::sync::oneshot::channel();
        let tx = Arc::new(Mutex::new(Some(tx)));
        if let Err(tincan::SendError(e)) =
            self.downstream.send(Downstream::Info(ConnectedPeers(tx)))
        {
            match e {
                Downstream::Info(ConnectedPeers(reply)) => {
                    reply
                        .lock()
                        .take()
                        .expect("if chan send failed, there can't be another contender")
                        .send(vec![])
                        .ok();
                },

                _ => unreachable!(),
            }
        }

        rx.await.unwrap_or_default()
    }

    pub async fn stats(&self) -> event::downstream::Stats {
        use event::{
            downstream::{Info::*, Stats},
            Downstream,
        };

        let (tx, rx) = tokio::sync::oneshot::channel();
        let tx = Arc::new(Mutex::new(Some(tx)));
        if let Err(tincan::SendError(e)) = self.downstream.send(Downstream::Info(Stats(tx))) {
            match e {
                Downstream::Info(Stats(reply)) => {
                    reply
                        .lock()
                        .take()
                        .expect("if chan send failed, there can't be another contender")
                        .send(Stats::default())
                        .ok();
                },

                _ => unreachable!(),
            }
        }

        rx.await.unwrap_or_default()
    }

    pub fn graft(&self) -> Graft {
        Graft(self.downstream.clone())
    }

    pub fn subscribe(&self) -> impl futures::Stream<Item = Result<event::Upstream, RecvError>> {
        self.upstream.subscribe()
    }

    pub(self) fn emit(&self, evt: impl Into<event::Upstream>) {
        self.upstream.send(evt.into()).ok();
    }
}

impl Default for TinCans {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Graft(tincan::Sender<event::Downstream>);

impl Graft {
    /// Initiate a [`graft`] exchange with the specified peer.
    ///
    /// It should rarely be needed to initiate a graft explicitly, instead of
    /// relying on protocol defaults. Note also that namespace filter is not
    /// recalculated when calling this method.
    ///
    /// [`graft`]: ./graft/index.html
    pub async fn initiate<Addrs>(
        &self,
        remote_id: PeerId,
        addr_hints: Addrs,
    ) -> Result<(), error::GraftInitiate>
    where
        Addrs: IntoIterator<Item = SocketAddr>,
    {
        use event::{downstream::Graft::Initiate, Downstream};

        let (tx, rx) = tokio::sync::oneshot::channel();
        let reply = Arc::new(Mutex::new(Some(tx)));
        self.0
            .send(Downstream::Graft(Initiate {
                remote_id,
                addr_hints: addr_hints.into_iter().collect(),
                reply,
            }))
            .or(Err(error::GraftInitiate::Unavailable))?;

        rx.await.or(Err(error::GraftInitiate::Unavailable))?
    }

    /// Reset the [`graft`]ing state.
    ///
    /// Namely, recalculate the namespace filter from persistent storage, and
    /// reset the timer to the configured duration. Iow, prolong the sync period
    /// for another round.
    ///
    /// [`graft`]: ./graft/index.html
    pub async fn reset(
        &self,
        when: event::downstream::GraftResetPolicy,
    ) -> Result<(), error::GraftReset> {
        use event::{downstream::Graft::Reset, Downstream};

        let (tx, rx) = tokio::sync::oneshot::channel();
        let reply = Arc::new(Mutex::new(Some(tx)));
        self.0
            .send(Downstream::Graft(Reset { when, reply }))
            .or(Err(error::GraftReset::Unavailable))?;

        rx.await.or(Err(error::GraftReset::Unavailable))?
    }
}

pub async fn bind<Sign, Store>(
    phone: TinCans,
    config: Config,
    signer: Sign,
    storage: Store,
) -> Result<Bound<Store>, error::Bootstrap>
where
    Sign: Signer + Clone + Send + Sync + 'static,
    Store: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
{
    let local_id = PeerId::from_signer(&signer);
    let git = GitServer::new(&config.paths);
    let quic::BoundEndpoint { endpoint, incoming } =
        quic::Endpoint::bind(signer, config.listen_addr, config.network).await?;
    let (membership, periodic) = membership::Hpv::<_, SocketAddr>::new(
        local_id,
        Pcg64Mcg::new(rand::random()),
        config.membership,
    );
    let storage = Storage::from(storage);
    let graft = {
        let git = storage.get().await?;
        Arc::new(RwLock::new(graft::State::new(git.as_ref(), config.graft)?))
    };
    let state = State {
        local_id,
        endpoint,
        git,
        membership,
        storage,
        phone: phone.clone(),
        graft,
    };

    Ok(Bound {
        phone,
        state,
        incoming,
        periodic,
    })
}

pub fn accept<Store, Disco>(
    Bound {
        phone,
        state,
        incoming,
        periodic,
    }: Bound<Store>,
    disco: Disco,
) -> impl Future<Output = Result<!, quic::Error>>
where
    Store: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
    Disco: futures::Stream<Item = (PeerId, Vec<SocketAddr>)> + Send + 'static,
{
    let _git_factory = Arc::new(Box::new(state.clone()) as Box<dyn GitStreamFactory>);
    git::p2p::transport::register()
        .register_stream_factory(state.local_id, Arc::downgrade(&_git_factory));

    let tasks = [
        {
            let (fut, hdl) = future::abortable(accept::disco(state.clone(), disco));
            tokio::spawn(fut);
            hdl
        },
        {
            let (fut, hdl) = future::abortable(accept::periodic(state.clone(), periodic));
            tokio::spawn(fut);
            hdl
        },
        {
            let (fut, hdl) = future::abortable(accept::ground_control(
                state.clone(),
                phone.downstream.subscribe().into_stream().boxed(),
            ));
            tokio::spawn(fut);
            hdl
        },
    ];
    let main = io::ingress_connections(state.clone(), incoming).boxed();

    Accept {
        _git_factory,
        endpoint: state.endpoint,
        tasks,
        main,
    }
}

struct Accept {
    _git_factory: Arc<Box<dyn GitStreamFactory>>,
    endpoint: quic::Endpoint,
    tasks: [future::AbortHandle; 3],
    main: BoxFuture<'static, Result<!, quic::Error>>,
}

impl Future for Accept {
    type Output = Result<!, quic::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        self.main.poll_unpin(cx)
    }
}

impl Drop for Accept {
    fn drop(&mut self) {
        self.endpoint.shutdown();
        for task in &self.tasks {
            task.abort()
        }
    }
}

pub trait ProtocolStorage<A>: broadcast::LocalStorage<A> + storage::Pooled + Send + Sync {}
impl<A, T> ProtocolStorage<A> for T where
    T: broadcast::LocalStorage<A> + storage::Pooled + Send + Sync
{
}

type Limiter = governor::RateLimiter<
    governor::state::direct::NotKeyed,
    governor::state::InMemoryState,
    governor::clock::DefaultClock,
>;

#[derive(Clone)]
struct Storage<S> {
    inner: S,
    limiter: Arc<Limiter>,
}

impl<S> From<S> for Storage<S> {
    fn from(inner: S) -> Self {
        Self {
            inner,
            limiter: Arc::new(RateLimiter::direct(Quota::per_second(
                // TODO: make this an "advanced" config
                nonzero!(5u32),
            ))),
        }
    }
}

impl<S> Deref for Storage<S> {
    type Target = S;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[async_trait]
impl<A, S> broadcast::LocalStorage<A> for Storage<S>
where
    A: 'static,
    S: broadcast::LocalStorage<A>,
    S::Update: Send,
{
    type Update = S::Update;

    async fn put<P>(&self, provider: P, has: Self::Update) -> broadcast::PutResult<Self::Update>
    where
        P: Into<(PeerId, Vec<A>)> + Send,
    {
        self.inner.put(provider, has).await
    }

    async fn ask(&self, want: Self::Update) -> bool {
        self.inner.ask(want).await
    }
}

impl<S> broadcast::ErrorRateLimited for Storage<S> {
    fn is_error_rate_limit_breached(&self) -> bool {
        self.limiter.check().is_err()
    }
}

#[async_trait]
impl<S> storage::Pooled for Storage<S>
where
    S: storage::Pooled + Send + Sync,
{
    async fn get(&self) -> Result<PooledRef, PoolError> {
        self.inner.get().await
    }
}

#[derive(Clone)]
struct State<S> {
    local_id: PeerId,
    endpoint: quic::Endpoint,
    git: GitServer,
    membership: membership::Hpv<Pcg64Mcg, SocketAddr>,
    storage: Storage<S>,
    phone: TinCans,
    graft: Arc<RwLock<graft::State>>,
}

#[async_trait]
impl<S> GitStreamFactory for State<S>
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
{
    async fn open_stream(
        &self,
        to: &PeerId,
        addr_hints: &[SocketAddr],
    ) -> Option<Box<dyn GitStream>> {
        let span = tracing::info_span!("open-git-stream", remote_id = %to);

        let may_conn = match self.endpoint.get_connection(*to) {
            Some(conn) => Some(conn),
            None => {
                let addr_hints = addr_hints.iter().copied().collect::<Vec<_>>();
                io::connect(&self.endpoint, *to, addr_hints)
                    .instrument(span.clone())
                    .await
                    .map(|(conn, ingress)| {
                        tokio::spawn(io::ingress_streams(self.clone(), ingress));
                        conn
                    })
            },
        };

        match may_conn {
            None => {
                span.in_scope(|| tracing::error!("unable to obtain connection"));
                None
            },

            Some(conn) => {
                let stream = conn
                    .open_bidi()
                    .inspect_err(|e| tracing::error!(err = ?e, "unable to open stream"))
                    .instrument(span.clone())
                    .await
                    .ok()?;
                let upgraded = upgrade::upgrade(stream, upgrade::Git)
                    .inspect_err(|e| tracing::error!(err = ?e, "unable to upgrade stream"))
                    .instrument(span)
                    .await
                    .ok()?;

                Some(Box::new(upgraded))
            },
        }
    }
}

impl<R, A> broadcast::Membership for membership::Hpv<R, A>
where
    R: rand::Rng + Clone,
    A: Clone + Debug + Ord,
{
    fn members(&self, exclude: Option<PeerId>) -> Vec<PeerId> {
        self.broadcast_recipients(exclude)
    }

    fn is_member(&self, peer: &PeerId) -> bool {
        self.is_active(peer)
    }
}
