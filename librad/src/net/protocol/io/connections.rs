// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::net::SocketAddr;

use either::Either;
use futures::{
    future::{self, TryFutureExt as _},
    stream::{Stream, StreamExt as _},
};
use indexmap::IndexSet;

use crate::{
    net::{
        protocol::{
            event::upstream as event,
            gossip,
            info::PeerInfo,
            io::streams,
            ProtocolStorage,
            State,
        },
        quic,
    },
    PeerId,
};

/// Dispatch incoming connections and streams.
///
/// # Panics
///
/// Panics if one of the tasks spawned by this function panics.
#[tracing::instrument(skip(state, ingress))]
pub(in crate::net::protocol) async fn incoming<S, I>(
    state: State<S>,
    ingress: I,
) -> Result<!, quic::Error>
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
    I: futures::Stream<
        Item = quic::Result<(quic::Connection, quic::BoxedIncomingStreams<'static>)>,
    >,
{
    use quic::Error::*;

    let listen_addrs = state.endpoint.listen_addrs();
    state.phone.emit(event::Endpoint::Up { listen_addrs });

    let ingress = ingress.fuse();
    futures::pin_mut!(ingress);
    while let Some(conn) = ingress.next().await {
        match conn {
            Ok((_, streams)) => {
                state
                    .spawner
                    .spawn(streams::incoming(state.clone(), streams))
                    .detach();
            },
            Err(err) => match err {
                Connection(_) | PeerId(_) | RemoteIdUnavailable | SelfConnect => {
                    tracing::warn!(err = %err, "ingress connections error");
                },
                Connect(_) | Endpoint(_) | Io(_) | Shutdown | Signer(_) | Task(_) => {
                    tracing::error!(err = %err, "ingress connections error");
                    break;
                },
            },
        }
    }

    tracing::debug!("ingress connections done, shutting down...");
    state.endpoint.wait_idle().await;
    tracing::debug!("shut down");

    Err(quic::Error::Shutdown)
}

pub async fn connect_peer_info(
    endpoint: &quic::Endpoint,
    peer_info: PeerInfo<SocketAddr>,
) -> Option<(
    quic::Connection,
    quic::IncomingStreams<
        impl Stream<Item = quic::Result<Either<quic::BidiStream, quic::RecvStream>>>,
    >,
)> {
    let addrs = peer_info
        .seen_addrs
        .into_iter()
        .chain(peer_info.advertised_info.listen_addrs);
    connect(endpoint, peer_info.peer_id, addrs).await
}

#[tracing::instrument(skip(endpoint, addrs))]
pub async fn connect<'a, Addrs>(
    endpoint: &quic::Endpoint,
    remote_id: PeerId,
    addrs: Addrs,
) -> Option<(
    quic::Connection,
    quic::IncomingStreams<
        impl Stream<Item = quic::Result<Either<quic::BidiStream, quic::RecvStream>>>,
    >,
)>
where
    Addrs: IntoIterator<Item = SocketAddr>,
{
    fn routable(addr: &SocketAddr) -> bool {
        let ip = addr.ip();
        !(ip.is_unspecified() || ip.is_documentation() || ip.is_multicast())
    }

    let addrs = addrs.into_iter().filter(routable).collect::<IndexSet<_>>();
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
