// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{cmp, collections::BTreeSet, convert::TryFrom, io, net::SocketAddr};

use futures::{
    future::{self, TryFutureExt as _},
    io::{AsyncRead, AsyncWrite},
    sink::SinkExt as _,
    stream::{self, StreamExt as _, TryStreamExt as _},
};
use futures_codec::{Framed, FramedRead, FramedWrite};
use thiserror::Error;

use super::{
    broadcast,
    error,
    event::upstream as event,
    gossip,
    info::{PartialPeerInfo, PeerAdvertisement, PeerInfo},
    membership,
    syn,
    tick,
    ProtocolStorage,
    State,
};
use crate::{
    git::storage::pool::{PoolError, PooledStorage},
    identities::SomeUrn,
    net::{
        codec::{CborCodec, CborCodecError, CborError},
        connection::{CloseReason, Duplex as _, RemoteAddr as _, RemoteInfo, RemotePeer},
        quic,
        upgrade::{self, Upgraded},
    },
    PeerId,
};

type Codec<T> = CborCodec<T, T>;
type GossipCodec<T> = Codec<broadcast::Message<SocketAddr, T>>;
type MembershipCodec = Codec<membership::Message<SocketAddr>>;
type IngressSynCodec = CborCodec<syn::Response, syn::Request>;
type EgressSynCodec = CborCodec<syn::Request, syn::Response>;

#[tracing::instrument(skip(state, peer, addrs), fields(remote_id = %peer))]
pub(super) async fn discovered<S>(state: State<S>, peer: PeerId, addrs: Vec<SocketAddr>)
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
{
    if state.endpoint.get_connection(peer).is_some() {
        return;
    }

    if let Some((conn, ingress)) = connect(&state.endpoint, peer, addrs).await {
        let rpc_sent = send_rpc::<_, ()>(
            &conn,
            state.membership.hello(peer_advertisement(&state.endpoint)),
        )
        .await;

        match rpc_sent {
            Err(e) => tracing::warn!(err = ?e, "failed to send membership hello"),
            Ok(()) => {
                let info = || peer_advertisement(&state.endpoint);
                let membership::TnT { trans, ticks } =
                    state.membership.connection_established(PartialPeerInfo {
                        peer_id: peer,
                        advertised_info: None,
                        seen_addrs: vec![conn.remote_addr()].into_iter().collect(),
                    });

                trans.into_iter().for_each(|evt| state.events.emit(evt));
                for tick in ticks {
                    stream::iter(membership::collect_tocks(&state.membership, &info, tick))
                        .for_each(|tock| tick::tock(state.clone(), tock))
                        .await
                }

                tokio::spawn(ingress_streams(state.clone(), ingress));
            },
        }

        tokio::spawn(async {
            let state_here = state;
            initiate_sync(&state_here, conn).await
        });
    }
}

#[tracing::instrument(skip(state, ingress), err)]
pub(super) async fn ingress_connections<S, I>(
    state: State<S>,
    mut ingress: I,
) -> Result<!, quic::Error>
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
    I: futures::Stream<Item = quic::Result<(quic::Connection, quic::IncomingStreams<'static>)>>
        + Unpin,
{
    let listen_addrs = state.endpoint.listen_addrs()?.collect();
    state.events.emit(event::Endpoint::Up { listen_addrs });

    while let Some((_, streams)) = ingress.try_next().await? {
        tokio::spawn(ingress_streams(state.clone(), streams));
    }

    state.events.emit(event::Endpoint::Down);
    Err(quic::Error::Shutdown)
}

#[tracing::instrument(skip(state, bidi, uni))]
pub(super) async fn ingress_streams<S>(
    state: State<S>,
    quic::IncomingStreams { bidi, uni }: quic::IncomingStreams<'static>,
) where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
{
    let mut bidi = bidi
        .inspect_ok(|stream| {
            tracing::info!(
                remote_id = %stream.remote_peer_id(),
                remote_addr = %stream.remote_addr(),
                "new ingress bidi stream"
            )
        })
        .fuse();
    let mut uni = uni
        .inspect_ok(|stream| {
            tracing::info!(
                remote_id = %stream.remote_peer_id(),
                remote_addr = %stream.remote_addr(),
                "new ingress uni stream"
            )
        })
        .fuse();

    loop {
        futures::select! {
            stream = bidi.next() => match stream {
                Some(item) => match item {
                    Ok(stream) => ingress_bidi(state.clone(), stream).await,
                    Err(e) => {
                        tracing::warn!(err = ?e, "ingress bidi error");
                        break
                    }
                },
                None => {
                    break
                }
            },
            stream = uni.next() => match stream {
                Some(item) => match item {
                    Ok(stream) => ingress_uni(state.clone(), stream).await,
                    Err(e) => {
                        tracing::warn!(err = ?e, "ingress uni error");
                        break
                    }
                },
                None => {
                    break
                }
            },
            complete => break
        }
    }
    tracing::debug!("ingress_streams done");
}

pub(super) async fn ingress_bidi<S>(state: State<S>, stream: quic::BidiStream)
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
{
    use upgrade::SomeUpgraded::*;

    match upgrade::with_upgraded(stream).await {
        Err(upgrade::Error { stream, source }) => {
            tracing::warn!(err = ?source, "invalid upgrade");
            stream.close(CloseReason::InvalidUpgrade)
        },

        Ok(Git(up)) => {
            if let Err(e) = state.git.invoke_service(up.into_stream().split()).await {
                tracing::warn!(err = ?e, "git service error");
            }
        },

        Ok(Gossip(up)) => ingress_gossip(state, up).await,
        Ok(Membership(up)) => ingress_membership(state, up).await,
        Ok(Syn(up)) => ingress_syn(state, up).await,
    }
}

pub(super) async fn ingress_uni<S>(state: State<S>, stream: quic::RecvStream)
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
{
    use upgrade::SomeUpgraded::*;

    match upgrade::with_upgraded(stream).await {
        Err(upgrade::Error { stream, source }) => {
            tracing::warn!(err = ?source, "invalid upgrade");
            stream.close(CloseReason::InvalidUpgrade)
        },

        Ok(Git(up)) => {
            tracing::warn!("unidirectional git requested");
            up.into_stream().close(CloseReason::InvalidUpgrade);
        },
        Ok(Syn(up)) => {
            tracing::warn!("unidirectional syn requested");
            up.into_stream().close(CloseReason::InvalidUpgrade);
        },

        Ok(Gossip(up)) => ingress_gossip(state, up).await,
        Ok(Membership(up)) => ingress_membership(state, up).await,
    }
}

async fn ingress_gossip<S, T>(state: State<S>, stream: Upgraded<upgrade::Gossip, T>)
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
    T: RemotePeer + AsyncRead + Unpin,
{
    let mut recv = FramedRead::new(stream.into_stream(), GossipCodec::new());
    let remote_id = recv.remote_peer_id();

    while let Some(x) = recv.next().await {
        match x {
            Err(e) => {
                tracing::warn!(err = ?e, "gossip recv error");
                let info = || peer_advertisement(&state.endpoint);

                let membership::TnT { trans, ticks } = state.membership.connection_lost(remote_id);
                trans.into_iter().for_each(|evt| state.events.emit(evt));
                for tick in ticks {
                    stream::iter(membership::collect_tocks(&state.membership, &info, tick))
                        .for_each(|tock| tick::tock(state.clone(), tock))
                        .await
                }

                break;
            },

            Ok(msg) => {
                let peer_info = || PeerInfo {
                    peer_id: state.local_id,
                    advertised_info: peer_advertisement(&state.endpoint),
                    seen_addrs: Default::default(),
                };
                match broadcast::apply(
                    &state.membership,
                    &state.storage,
                    &peer_info,
                    remote_id,
                    msg,
                )
                .await
                {
                    Err(e) => {
                        tracing::warn!(err = ?e, "gossip error");
                        break;
                    },

                    Ok((may_event, tocks)) => {
                        if let Some(event) = may_event {
                            state.events.emit(event)
                        }

                        stream::iter(tocks)
                            .for_each(|tock| tick::tock(state.clone(), tock))
                            .await
                    },
                }
            },
        }
    }
}

async fn ingress_membership<S, T>(state: State<S>, stream: Upgraded<upgrade::Membership, T>)
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
    T: RemoteInfo<Addr = SocketAddr> + AsyncRead + Unpin,
{
    let mut recv = FramedRead::new(stream.into_stream(), MembershipCodec::new());
    let remote_id = recv.remote_peer_id();
    let remote_addr = recv.remote_addr();

    while let Some(x) = recv.next().await {
        match x {
            Err(e) => {
                tracing::warn!(err = ?e, "membership recv error");
                let info = || peer_advertisement(&state.endpoint);

                let membership::TnT { trans, ticks } = state.membership.connection_lost(remote_id);
                trans.into_iter().for_each(|evt| state.events.emit(evt));
                for tick in ticks {
                    stream::iter(membership::collect_tocks(&state.membership, &info, tick))
                        .for_each(|tock| tick::tock(state.clone(), tock))
                        .await
                }

                break;
            },

            Ok(msg) => {
                let info = || peer_advertisement(&state.endpoint);
                match membership::apply(&state.membership, &info, remote_id, remote_addr, msg) {
                    Err(e) => {
                        tracing::warn!(err = ?e, "membership error");
                        break;
                    },

                    Ok((trans, tocks)) => {
                        trans.into_iter().for_each(|evt| state.events.emit(evt));
                        stream::iter(tocks)
                            .for_each(|tock| tick::tock(state.clone(), tock))
                            .await
                    },
                }
            },
        }
    }
}

async fn ingress_syn<S, T>(state: State<S>, stream: Upgraded<upgrade::Syn, T>)
where
    S: PooledStorage,
    T: AsyncRead + AsyncWrite + Unpin,
{
    #[derive(Debug, Error)]
    enum Error {
        #[error("error handling request")]
        Handler(#[from] syn::error::Request),

        #[error("unable to borrow pooled storage")]
        Pool(#[from] PoolError),

        #[error(transparent)]
        Cbor(#[from] CborError),

        #[error(transparent)]
        Io(#[from] io::Error),
    }

    impl From<CborCodecError> for Error {
        fn from(e: CborCodecError) -> Self {
            match e {
                CborCodecError::Cbor(e) => Self::Cbor(e),
                CborCodecError::Io(e) => Self::Io(e),
            }
        }
    }

    async fn go<S, T>(state: State<S>, mut framing: Framed<T, IngressSynCodec>) -> Result<(), Error>
    where
        S: PooledStorage,
        T: AsyncRead + AsyncWrite + Unpin,
    {
        if let Some(req) = framing.try_next().await? {
            let storage = state.storage.get().await?;
            for resp in syn::handle_request(storage, req).await? {
                framing.send(resp).await?
            }
        }

        Ok(())
    }

    let framing = Framed::new(stream.into_stream(), IngressSynCodec::new());
    if let Err(e) = go(state, framing).await {
        tracing::error!(err = ?e, "{}", e)
    }
}

pub(super) async fn connect_peer_info<'a>(
    endpoint: &quic::Endpoint,
    peer_info: PeerInfo<SocketAddr>,
) -> Option<(quic::Connection, quic::IncomingStreams<'a>)> {
    let addrs = peer_info
        .advertised_info
        .listen_addrs
        .into_iter()
        .chain(peer_info.seen_addrs.into_iter());
    connect(endpoint, peer_info.peer_id, addrs).await
}

#[tracing::instrument(skip(endpoint, addrs))]
pub(super) async fn connect<'a, Addrs>(
    endpoint: &quic::Endpoint,
    remote_id: PeerId,
    addrs: Addrs,
) -> Option<(quic::Connection, quic::IncomingStreams<'a>)>
where
    Addrs: IntoIterator<Item = SocketAddr>,
{
    fn routable(addr: &SocketAddr) -> bool {
        let ip = addr.ip();
        !(ip.is_unspecified() || ip.is_documentation() || ip.is_multicast())
    }

    let addrs = addrs.into_iter().filter(routable).collect::<BTreeSet<_>>();
    if addrs.is_empty() {
        tracing::warn!("no routable addrs");
        None
    } else {
        future::select_ok(addrs.iter().map(|addr| {
            let mut endpoint = endpoint.clone();
            tracing::info!(remote_addr = %addr, "establishing connection");
            Box::pin(async move {
                endpoint
                    .connect(remote_id, &addr)
                    .map_err(|e| {
                        tracing::warn!(err = ?e, remote_addr = %addr, "could not connect");
                        e
                    })
                    .await
            })
        }))
        .await
        .ok()
        .map(|(success, _pending)| success)
    }
}

pub(super) fn peer_advertisement(endpoint: &quic::Endpoint) -> PeerAdvertisement<SocketAddr> {
    let addrs = endpoint
        .listen_addrs()
        .expect("unable to obtain listen addrs");
    PeerAdvertisement {
        listen_addrs: addrs.collect(),
        capabilities: Default::default(),
    }
}

#[derive(Debug)]
pub(super) enum Rpc<A, P>
where
    A: Clone + Ord,
{
    Membership(membership::Message<A>),
    Gossip(broadcast::Message<A, P>),
}

impl<A, P> From<membership::Message<A>> for Rpc<A, P>
where
    A: Clone + Ord,
{
    fn from(m: membership::Message<A>) -> Self {
        Self::Membership(m)
    }
}

impl<A, P> From<broadcast::Message<A, P>> for Rpc<A, P>
where
    A: Clone + Ord,
{
    fn from(m: broadcast::Message<A, P>) -> Self {
        Self::Gossip(m)
    }
}

#[allow(clippy::unit_arg)]
#[tracing::instrument(
    skip(conn, rpc),
    fields(
        remote_id = %conn.remote_peer_id(),
        remote_addr = %conn.remote_addr()
    ),
    err
)]
pub(super) async fn send_rpc<R, P>(conn: &quic::Connection, rpc: R) -> Result<(), error::SendGossip>
where
    R: Into<Rpc<SocketAddr, P>>,
    P: minicbor::Encode,
{
    use Rpc::*;

    let stream = conn.open_uni().await?;

    match rpc.into() {
        Membership(msg) => {
            let upgraded = upgrade::upgrade(stream, upgrade::Membership).await?;
            FramedWrite::new(upgraded, MembershipCodec::new())
                .send(msg)
                .await?;
            Ok(())
        },

        Gossip(msg) => {
            let upgraded = upgrade::upgrade(stream, upgrade::Gossip).await?;
            FramedWrite::new(upgraded, GossipCodec::new())
                .send(msg)
                .await?;
            Ok(())
        },
    }
}

#[allow(clippy::unit_arg)]
#[tracing::instrument(
    skip(state, conn),
    fields(
        remote_id = %conn.remote_peer_id(),
        remote_addr = %conn.remote_addr()
    ),
    err
)]
pub(super) async fn initiate_sync<S>(
    state: &State<S>,
    conn: quic::Connection,
) -> Result<(), error::InitiateSync>
where
    S: PooledStorage + Send + Sync + 'static,
{
    let remote_id = conn.remote_peer_id();
    let remote_addr = conn.remote_addr();

    let mut framing = {
        let bi = conn.open_bidi().await?;
        let stream = upgrade::upgrade(bi, upgrade::Syn).await?.into_stream();
        Framed::new(stream, EgressSynCodec::new())
    };

    let (num_requested, filter) = {
        let sync = state.sync.read();
        match sync.snapshot() {
            None => Ok((0, None)),
            Some(bloom) => {
                let n = cmp::min(bloom.approx_elements(), syn::MAX_OFFER_TOTAL);
                let f =
                    syn::rpc::BloomFilter::try_from(bloom).map_err(error::InitiateSync::Bloom)?;
                Ok::<_, error::InitiateSync>((n, Some(f)))
            },
        }
    }?;
    if num_requested == 0 {
        tracing::info!("zero filter, not syncing");
        return Ok(());
    }

    tracing::info!(num_requested, "requesting sync info");
    framing
        .send(syn::Request::ListNamespaces { filter })
        .await?;

    // TODO: timeout
    framing
        .take(num_requested / syn::rpc::MAX_OFFER_BATCH_SIZE)
        .map_ok(|resp| {
            syn::handle_response(&state.storage, resp, remote_id, remote_addr)
                .map_err(error::InitiateSync::from)
        })
        .try_flatten()
        .try_for_each(|SomeUrn::Git(urn)| {
            tracing::info!("synced {}", urn);
            future::ok(())
        })
        .await
}
