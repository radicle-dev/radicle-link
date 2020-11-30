// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! Simplified implementation of the seminal [Epidemic Broadcast Trees] paper
//! and accompanying [HyParView] membership protocol.
//!
//! [Epidemic Broadcast Trees]: http://asc.di.fct.unl.pt/~jleitao/pdf/srds07-leitao.pdf
//! [HyParView]: http://asc.di.fct.unl.pt/~jleitao/pdf/dsn07-leitao.pdf

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Debug,
    marker::PhantomData,
    num::NonZeroU32,
    ops::Deref as _,
    sync::{
        atomic::{self, AtomicUsize},
        Arc,
    },
    time::Duration,
};

use futures::{
    channel::mpsc,
    future::{self, BoxFuture, FutureExt as _},
    io::{AsyncRead, AsyncWrite},
    sink::SinkExt as _,
    stream::{FuturesUnordered, StreamExt as _, TryStreamExt as _},
};
use futures_codec::{Framed, FramedRead, FramedWrite};
use futures_timer::Delay;
use governor::{Quota, RateLimiter};
use minicbor::{Decode, Encode};
use rand::Rng as _;
use rand_pcg::Pcg64Mcg;

use crate::{
    internal::channel::Fanout,
    net::{
        codec::{CborCodec, CborCodecError},
        connection::{AsAddr, Duplex, HasStableId, RemoteInfo, RemotePeer as _},
        conntrack::{self, SyncStream},
        gossip::error::Error,
        upgrade::{self, Upgraded},
    },
    peer::PeerId,
};

pub mod error;

mod rpc;
pub use rpc::{Gossip, Membership, Rpc};

mod storage;
pub use storage::{LocalStorage, PutResult};

mod types;
pub use types::{Capability, PartialPeerInfo, PeerAdvertisement, PeerInfo};

mod membership;
pub use membership::MembershipParams;

#[derive(Clone)]
pub enum ProtocolEvent<Addr, Payload>
where
    Addr: Clone + Ord,
{
    Control(Control<Addr, Payload>),
    Info(Info<Addr, Payload>),
}

#[derive(Clone)]
pub enum Control<Addr, Payload>
where
    Addr: Clone + Ord,
{
    SendAdhoc {
        to: PeerInfo<Addr>,
        rpc: Rpc<Addr, Payload>,
    },
    Connect {
        to: PeerInfo<Addr>,
    },
    Disconnect {
        peer: PeerId,
    },
}

#[derive(Clone, Debug)]
pub enum Info<Addr, A>
where
    Addr: Clone + Ord,
{
    Has(Has<Addr, A>),
}

#[derive(Clone, Debug)]
pub struct Has<Addr, A>
where
    Addr: Clone + Ord,
{
    pub provider: PeerInfo<Addr>,
    pub val: A,
}

pub type Codec<A, P> = CborCodec<Rpc<A, P>, Rpc<A, P>>;
type WriteStream<W, A, P> = FramedWrite<W, Codec<A, P>>;
type StorageErrorLimiter = RateLimiter<
    governor::state::direct::NotKeyed,
    governor::state::InMemoryState,
    governor::clock::DefaultClock,
>;

pub struct Protocol<Storage, Broadcast, Addr, Stream>
where
    Addr: Clone + Ord,
    Stream: Duplex + HasStableId,
    Stream::Write: HasStableId,
{
    local_id: PeerId,
    local_ad: PeerAdvertisement<Addr>,

    storage: Storage,
    storage_error_lim: Arc<StorageErrorLimiter>,

    subscribers: Fanout<ProtocolEvent<Addr, Broadcast>>,

    streams: conntrack::Streams<WriteStream<Stream::Write, Addr, Broadcast>>,
    membership: membership::Hpv<Pcg64Mcg, Stream::Id, Addr>,

    ref_count: Arc<AtomicUsize>,

    _read_stream: PhantomData<Stream::Read>,
}

impl<Storage, Broadcast, Addr, Stream> Clone for Protocol<Storage, Broadcast, Addr, Stream>
where
    Storage: Clone,
    Broadcast: Clone,
    Addr: Clone + Ord,
    Stream: Duplex + HasStableId,
    Stream::Write: HasStableId,
{
    fn clone(&self) -> Self {
        const MAX_REFCOUNT: usize = (isize::MAX) as usize;

        let old_count = self.ref_count.fetch_add(1, atomic::Ordering::Relaxed);
        // See source docs of `std::sync::Arc`
        if old_count > MAX_REFCOUNT {
            eprintln!("Fatal: max refcount on gossip::Protocol exceeded");
            core::intrinsics::abort()
        }

        Self {
            local_id: self.local_id,
            local_ad: self.local_ad.clone(),
            storage: self.storage.clone(),
            storage_error_lim: self.storage_error_lim.clone(),
            subscribers: self.subscribers.clone(),
            streams: self.streams.clone(),
            membership: self.membership.clone(),
            ref_count: self.ref_count.clone(),
            _read_stream: self._read_stream,
        }
    }
}

impl<Storage, Broadcast, Addr, Stream> Protocol<Storage, Broadcast, Addr, Stream>
where
    Storage: LocalStorage<Update = Broadcast> + 'static,

    for<'de> Broadcast: Decode<'de>,
    for<'de> Addr: Decode<'de>,

    Broadcast: Encode + Clone + Debug + Send + Sync + 'static,
    Addr: Encode + Clone + Debug + Ord + Send + Sync + 'static,

    Stream: Duplex + HasStableId + 'static,
    Stream::Read: AsyncRead
        + HasStableId<Id = Stream::Id>
        + RemoteInfo<Addr = Stream::Addr>
        + Unpin
        + Send
        + Sync
        + 'static,
    Stream::Write: AsyncWrite
        + HasStableId<Id = Stream::Id>
        + RemoteInfo<Addr = Stream::Addr>
        + Unpin
        + Send
        + Sync
        + 'static,
    Stream::Addr: AsAddr<Addr>,
    Stream::Id: Clone + Debug + Ord + Send + Sync + 'static,
{
    pub fn new(
        local_id: PeerId,
        local_ad: PeerAdvertisement<Addr>,
        mparams: MembershipParams,
        storage: Storage,
    ) -> Self {
        let prng = Pcg64Mcg::new(rand::random());
        let storage_error_lim = Arc::new(RateLimiter::direct(Quota::per_second(unsafe {
            NonZeroU32::new_unchecked(5)
        })));

        let shuffle_interval = mparams.shuffle_interval;
        let promote_interval = mparams.promote_interval;

        let streams = conntrack::Streams::default();
        let membership = membership::Hpv::new(local_id, prng, mparams);

        let this = Self {
            local_id,
            local_ad,
            storage,
            storage_error_lim,
            subscribers: Fanout::new(),
            streams,
            membership,
            ref_count: Arc::new(AtomicUsize::new(0)),
            _read_stream: PhantomData,
        };

        // Spawn periodic tasks, ensuring they complete when the last reference
        // to `this` is dropped.
        {
            let shuffle = this.clone();
            let promotion = this.clone();
            // We got two clones, so if the ref_count goes below that, we're the
            // only ones holding on to a reference of `this`.
            let ref_count = 2;
            tokio::spawn({
                async move {
                    let holdoff = {
                        let mut rng = rand::thread_rng();
                        rng.gen_range(0, shuffle_interval.as_secs())
                    };
                    Delay::new(Duration::from_secs(holdoff)).await;
                    loop {
                        if shuffle.ref_count.load(atomic::Ordering::Relaxed) < ref_count {
                            tracing::trace!("stopping periodic shuffle task");
                            break;
                        }
                        Delay::new(shuffle_interval).await;
                        shuffle.shuffle().await;
                    }
                }
            });
            tokio::spawn({
                async move {
                    let holdoff = {
                        let mut rng = rand::thread_rng();
                        rng.gen_range(0, promote_interval.as_secs())
                    };
                    Delay::new(Duration::from_secs(holdoff)).await;
                    loop {
                        if promotion.ref_count.load(atomic::Ordering::Relaxed) < ref_count {
                            tracing::trace!("stopping periodic promotion task");
                            break;
                        }
                        Delay::new(promote_interval).await;
                        promotion.promote_random().await;
                    }
                }
            });
        }

        this
    }

    pub fn peer_id(&self) -> PeerId {
        self.local_id
    }

    #[tracing::instrument(skip(self, have))]
    pub async fn announce(&self, have: Broadcast) {
        self.broadcast(
            Gossip::Have {
                origin: self.local_peer_info(),
                val: have,
            },
            None,
        )
        .await
    }

    #[tracing::instrument(skip(self, want))]
    pub async fn query(&self, want: Broadcast) {
        self.broadcast(
            Gossip::Want {
                origin: self.local_peer_info(),
                val: want,
            },
            None,
        )
        .await
    }

    pub(super) async fn subscribe(
        &self,
    ) -> mpsc::UnboundedReceiver<ProtocolEvent<Addr, Broadcast>> {
        self.subscribers.subscribe().await
    }

    fn is_connected(&self) -> bool {
        !self.streams.is_empty()
    }

    fn has_active(&self) -> bool {
        self.membership.num_active() > 0
    }

    #[allow(clippy::unit_arg)]
    #[tracing::instrument(level = "debug", skip(self, s, hello), err)]
    pub(super) async fn outgoing_bidi(
        &self,
        s: Upgraded<upgrade::Gossip, Stream>,
        hello: impl Into<Option<Rpc<Addr, Broadcast>>>,
    ) -> Result<(), Error> {
        let hello = match hello.into() {
            Some(rpc) => rpc,
            None => {
                let local_ad = self.local_ad.clone();
                if self.is_connected() {
                    Membership::Neighbour {
                        info: local_ad,
                        need_friends: self.has_active().then_some(()),
                    }
                } else {
                    Membership::Join(local_ad)
                }
                .into()
            },
        };

        let mut s = Framed::new(s, Codec::new());
        s.send(hello).await?;

        let tick = self.membership.connection_established(
            PartialPeerInfo {
                peer_id: s.remote_peer_id(),
                advertised_info: None,
                seen_addrs: Some(s.remote_addr().as_addr()).into_iter().collect(),
            },
            s.stable_id(),
        );
        if let Some(tock) = tick {
            self.clone().handle_membership_tick(tock).await
        }

        self.incoming_bidi(s.release().0).await
    }

    #[allow(clippy::unit_arg)]
    #[tracing::instrument(level = "debug", skip(self, s, rpc), err)]
    pub(super) async fn outgoing_uni(
        &self,
        s: Upgraded<upgrade::Gossip, Stream::Write>,
        rpc: Rpc<Addr, Broadcast>,
    ) -> Result<(), Error> {
        Ok(FramedWrite::new(s, Codec::new()).send(rpc).await?)
    }

    #[allow(clippy::unit_arg)]
    #[tracing::instrument(level = "debug", skip(self, s), err)]
    pub(super) async fn incoming_bidi(
        &self,
        s: Upgraded<upgrade::Gossip, Stream>,
    ) -> Result<(), Error> {
        let s = s.into_stream();
        let stream_id = s.stable_id();
        let (recv, send) = s.split();
        let recv = FramedRead::new(recv, Codec::new());
        let send = SyncStream::from(FramedWrite::new(send, Codec::new()));

        let remote_id = recv.remote_peer_id();
        // This should not be possible, as we prevent it in the TLS handshake.
        // Leaving it here regardless as a sanity check.
        if remote_id == self.local_id {
            return Err(Error::SelfConnection);
        }

        if let Some(prev) = self.streams.insert(send.clone()) {
            tracing::warn!(
                "incoming ejects previous stream {}: {:?}",
                remote_id,
                prev.stable_id()
            );
            let _ = prev.lock().await.close().await;
        }

        let remote_addr = recv.remote_addr().as_addr();
        let res = recv
            .map_err(Error::from)
            .and_then(|rpc| {
                let remote_addr = remote_addr.clone();
                async move {
                    match rpc {
                        Rpc::Membership(msg) => {
                            self.handle_membership(remote_id, remote_addr, stream_id, msg)
                                .await
                        },
                        Rpc::Gossip(msg) => self.handle_gossip(remote_id, stream_id, msg).await,
                    }
                }
            })
            .try_for_each(future::ok)
            .await;
        tracing::info!(peer = %remote_id, "recv stream is done");

        if res.is_err() {
            let was_removed = self.streams.remove(&send);
            let tick = if was_removed {
                let stream_id = send.stable_id();
                tracing::info!(peer = %remote_id, stream = ?stream_id, "closing send stream");
                let _ = send.lock().await.close().await;
                self.membership.connection_lost(remote_id, stream_id)
            } else {
                None
            };
            if let Some(tock) = tick {
                self.clone().handle_membership_tick(tock).await;
            }
        }

        res.or_else(|e| match e {
            // This is usually a connection close or reset. We only log upstream,
            // and it's not too interesting to get error / warn logs for this.
            Error::Cbor(CborCodecError::Io(_)) => {
                tracing::debug!("ignoring recv IO error: {}", e);
                Ok(())
            },
            e => Err(e),
        })
    }

    #[allow(clippy::unit_arg)]
    #[tracing::instrument(level = "debug", skip(self, s), err)]
    pub(super) async fn incoming_uni(
        &self,
        s: Upgraded<upgrade::Gossip, Stream::Read>,
    ) -> Result<(), Error> {
        let recv = FramedRead::new(s.into_stream(), Codec::<Addr, Broadcast>::new());
        let stream_id = recv.deref().stable_id();

        let remote_id = recv.remote_peer_id();
        if remote_id == self.local_id {
            return Err(Error::SelfConnection);
        }
        let remote_addr = recv.remote_addr().as_addr();

        recv.map_err(Error::from)
            .and_then(|rpc| {
                let remote_addr = remote_addr.clone();
                async move {
                    match rpc {
                        Rpc::Membership(msg) => match msg {
                            Membership::ShuffleReply { .. } => {
                                self.handle_membership(remote_id, remote_addr, stream_id, msg)
                                    .await
                            },
                            _ => Err(Error::ProtocolViolation(
                                "only shuffle reply messages are legal over uni streams",
                            )),
                        },

                        _ => Err(Error::ProtocolViolation(
                            "gossip can not be sent over a unidirectional stream",
                        )),
                    }
                }
            })
            .try_for_each(future::ok)
            .await
            .or_else(|e| match e {
                // This is usually a connection close or reset. We only log upstream,
                // and it's not too interesting to get error / warn logs for this.
                Error::Cbor(CborCodecError::Io(_)) => {
                    tracing::debug!("ignoring recv IO error: {}", e);
                    Ok(())
                },
                e => Err(e),
            })
    }

    #[allow(clippy::unit_arg)]
    #[tracing::instrument(level = "debug", skip(self, remote_id, msg), fields(remote_id = %remote_id), err)]
    async fn handle_membership(
        &self,
        remote_id: PeerId,
        remote_addr: Addr,
        stream_id: Stream::Id,
        msg: Membership<Addr>,
    ) -> Result<(), Error> {
        let tick = self
            .membership
            .apply(remote_id, remote_addr, stream_id, msg)
            .map_err(|e| {
                tracing::warn!("membership error: {}", e);
                Error::ProtocolViolation("membership protocol violation")
            })?;

        if let Some(tock) = tick {
            self.clone().handle_membership_tick(tock).await;
        }

        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self))]
    // boxing due to recursion, see:
    // https://rust-lang.github.io/async-book/07_workarounds/04_recursion.html
    fn handle_membership_tick(
        self,
        tick: membership::Tick<Stream::Id, Addr>,
    ) -> BoxFuture<'static, ()> {
        use membership::Tick::*;

        tracing::trace!("tick");
        async move {
            match tick {
                Ticks { ticks } => {
                    ticks
                        .into_iter()
                        .map(|tick| self.clone().handle_membership_tick(tick))
                        .collect::<FuturesUnordered<_>>()
                        .for_each(future::ready)
                        .await
                },
                Demote { peer, stream } => self.demote_stream(&peer, &stream).await,
                Forget { peer } => self.forget(peer).await,
                Connect { to } => self.connect(&to).await,
                Reply { to, message } => self.send_adhoc(to, message).await,
                Broadcast {
                    recipients,
                    message,
                } => self.send(recipients, Rpc::Membership(message)).await,
            }
        }
        .boxed()
    }

    #[tracing::instrument(level = "debug", skip(self))]
    async fn demote_stream(&self, peer: &PeerId, id: &Stream::Id) {
        let demoted = self.streams.remove_id(peer, id);
        match demoted {
            None => tracing::warn!("stream not found"),
            Some(stream) => {
                tracing::info!("demoting");
                let _ = stream.lock().await.close().await;
            },
        }
    }

    async fn forget(&self, peer: PeerId) {
        self.subscribers
            .emit(ProtocolEvent::Control(Control::Disconnect { peer }))
            .await
    }

    #[tracing::instrument(level = "debug", skip(self, recipients))]
    async fn send<R>(&self, recipients: R, message: Rpc<Addr, Broadcast>)
    where
        R: IntoIterator<Item = (PeerId, Stream::Id)>,
    {
        let recipients = recipients.into_iter().collect::<BTreeMap<_, _>>();
        if recipients.is_empty() {
            tracing::warn!("empty recipients list");
            return;
        }

        let streams = {
            self.streams
                .as_vec()
                .iter()
                .filter_map(|(peer_id, stream)| {
                    recipients.get(peer_id).map(|expected_stream_id| {
                        // FIXME: understand how there is an inconsistency window
                        let actual_stream_id = stream.stable_id();
                        if *expected_stream_id != actual_stream_id {
                            tracing::warn!(
                                peer = %peer_id,
                                stream.expected = ?expected_stream_id,
                                stream.actual = ?actual_stream_id,
                                "destination stream changed"
                            );
                        }
                        stream.clone()
                    })
                })
                .collect::<Vec<_>>()
        };

        if streams.is_empty() {
            tracing::warn!("no send streams")
        } else {
            streams
                .into_iter()
                .map(|stream| {
                    let message = message.clone();
                    async move {
                        let peer = stream.remote_peer_id();
                        let id = stream.stable_id();
                        tracing::info!(peer = %peer, stream = ?id, "stream send");
                        stream
                            .lock()
                            .await
                            .send(message)
                            .await
                            .map_err(|e| (peer, id, e))
                    }
                })
                .collect::<FuturesUnordered<_>>()
                .for_each_concurrent(None, |res| async {
                    if let Err((peer, stream, err)) = res {
                        self.on_send_error(peer, stream, err).await
                    }
                })
                .await
        }
    }

    async fn on_send_error(&self, peer: PeerId, stream: Stream::Id, err: CborCodecError) {
        tracing::warn!(err = %err, peer = %peer, stream = ?stream, "stream send error");
        future::join(
            async move { self.demote_stream(&peer, &stream).await },
            async {
                let cont_tick = self.membership.connection_lost(peer, stream);
                if let Some(tick) = cont_tick {
                    self.clone().handle_membership_tick(tick).await
                }
            },
        )
        .map(|_| ())
        .await
    }

    #[tracing::instrument(skip(self, message, exclude))]
    async fn broadcast(
        &self,
        message: Gossip<Addr, Broadcast>,
        exclude: impl Into<Option<PeerId>>,
    ) {
        let exclude = exclude.into();
        let recipients = self.membership.broadcast_recipients(exclude);
        self.send(recipients, message.into()).await
    }

    #[allow(clippy::unit_arg)]
    #[tracing::instrument(skip(self, msg), err)]
    async fn handle_gossip(
        &self,
        remote_id: PeerId,
        stream_id: Stream::Id,
        msg: Gossip<Addr, Broadcast>,
    ) -> Result<(), Error> {
        use Gossip::*;

        match msg {
            Have { origin, val } => {
                tracing::trace!(origin = %origin.peer_id, value = ?val, "Have");

                self.subscribers
                    .emit(ProtocolEvent::Info(Info::Has(Has {
                        provider: origin.clone(),
                        val: val.clone(),
                    })))
                    .await;

                match self.storage.put(remote_id, val.clone()).await {
                    // `val` was new, and is now fetched to local storage.
                    // Let connected peers know they can now fetch it from
                    // us.
                    PutResult::Applied(val) => {
                        tracing::info!(value = ?val, "announcing applied value");

                        self.broadcast(
                            Have {
                                origin: self.local_peer_info(),
                                val,
                            },
                            remote_id,
                        )
                        .await
                    },

                    // Meh. Request retransmission.
                    PutResult::Error => {
                        tracing::info!(value = ?val, "error applying value");

                        // Forward in any case
                        self.broadcast(
                            Have {
                                origin,
                                val: val.clone(),
                            },
                            remote_id,
                        )
                        .await;
                        // Exit if we're getting too many errors
                        self.storage_error_lim
                            .check()
                            .map_err(|_| Error::StorageErrorRateLimitExceeded)?;
                        // Request retransmission
                        // This could be optimised be enqueuing `val`s and
                        // sending them in batch later (deduplicating)
                        self.broadcast(
                            Want {
                                origin: self.local_peer_info(),
                                val,
                            },
                            None,
                        )
                        .await
                    },

                    // Not interesting, forward to others
                    PutResult::Uninteresting => {
                        tracing::info!(value = ?val, "value is uninteresting");
                        self.broadcast(Have { origin, val }, remote_id).await
                    },

                    // We are up-to-date, don't do anything
                    PutResult::Stale => {
                        tracing::info!(value = ?val, "value is up to date");
                    },
                }
            },

            Want { origin, val } => {
                tracing::trace!(origin = %origin.peer_id, value = ?val, "Want");

                let have = self.storage.ask(val.clone()).await;
                if have {
                    self.send(
                        Some((remote_id, stream_id)),
                        Have {
                            origin: self.local_peer_info(),
                            val,
                        }
                        .into(),
                    )
                    .await
                } else {
                    self.broadcast(Want { origin, val }, remote_id).await
                }
            },
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    async fn shuffle(&self) {
        let (active, passive) = self.membership.view_stats();
        tracing::info!(active, passive, "initiating shuffle");
        match self.membership.shuffle() {
            None => tracing::info!("nothing to shuffle"),
            Some(shuf) => {
                self.send(
                    Some(shuf.recipient),
                    Membership::Shuffle {
                        origin: self.local_peer_info(),
                        peers: shuf.sample,
                        ttl: shuf.ttl,
                    }
                    .into(),
                )
                .await
            },
        }
    }

    #[tracing::instrument(skip(self))]
    async fn promote_random(&self) {
        let (active, passive) = self.membership.view_stats();
        tracing::info!(active, passive, "initiating random promotion");
        let candidates = self.membership.choose_passive_to_promote();
        match candidates {
            None => tracing::info!("no promotion candidates found"),
            Some(candidates) => {
                tracing::info!(
                    "requesting promotion for {:?}",
                    candidates
                        .iter()
                        .map(|info| info.peer_id)
                        .collect::<Vec<_>>()
                );
                candidates
                    .into_iter()
                    .map(|info| async move { self.connect(&info).await })
                    .collect::<FuturesUnordered<_>>()
                    .for_each(future::ready)
                    .await
            },
        }
    }

    /// Try to establish an ad-hoc connection to `peer`, and send it `rpc`
    async fn send_adhoc<M: Into<Rpc<Addr, Broadcast>>>(&self, peer: PeerInfo<Addr>, rpc: M) {
        self.subscribers
            .emit(ProtocolEvent::Control(Control::SendAdhoc {
                to: peer,
                rpc: rpc.into(),
            }))
            .await
    }

    /// Try to establish a persistent to `peer`
    async fn connect(&self, peer: &PeerInfo<Addr>) {
        self.subscribers
            .emit(ProtocolEvent::Control(Control::Connect {
                to: peer.clone(),
            }))
            .await
    }

    fn local_peer_info(&self) -> PeerInfo<Addr> {
        PeerInfo {
            peer_id: self.local_id,
            advertised_info: self.local_ad.clone(),
            seen_addrs: BTreeSet::default(),
        }
    }
}

impl<Storage, Broadcast, Addr, Stream> Drop for Protocol<Storage, Broadcast, Addr, Stream>
where
    Addr: Clone + Ord,
    Stream: Duplex + HasStableId,
    Stream::Write: HasStableId,
{
    fn drop(&mut self) {
        // `Relaxed` is presumably ok here, because all we want is to not wrap
        // around, which `saturating_sub` guarantees
        let r = self.ref_count.fetch_update(
            atomic::Ordering::Relaxed,
            atomic::Ordering::Relaxed,
            |x| Some(x.saturating_sub(1)),
        );
        match r {
            Ok(x) | Err(x) if x == 0 => tracing::trace!("`gossip::Protocol` refcount is zero"),
            _ => {},
        }
    }
}
