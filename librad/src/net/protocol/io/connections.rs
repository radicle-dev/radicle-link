// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{net::SocketAddr, panic};

use either::Either;
use futures::{
    future::{self, TryFutureExt as _},
    stream::{FuturesUnordered, Stream, StreamExt as _},
};
use indexmap::IndexSet;

use crate::{
    net::{
        protocol::{self, event::upstream as event, gossip, io, ProtocolStorage, State},
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
    mut state: State<S>,
    ingress: I,
) -> Result<!, quic::Error>
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
    I: futures::Stream<
        Item = quic::Result<(quic::Connection, quic::BoxedIncomingStreams<'static>)>,
    >,
{
    use protocol::graft::Source;
    use quic::Error::*;

    let listen_addrs = state.endpoint.listen_addrs();
    state.phone.emit(event::Endpoint::Up { listen_addrs });

    let mut tasks = FuturesUnordered::new();
    let ingress = ingress.fuse();
    futures::pin_mut!(ingress);
    loop {
        futures::select! {
            conn = ingress.next() => match conn {
                Some(conn) => match conn {
                    Ok((conn, streams)) => {
                        state.graft_trigger(conn, Source::Incoming);
                        tasks.push(state.spawner.spawn(io::streams::incoming(state.clone(), streams)));
                    },
                    Err(err)=> match err {
                        Connection(_) | PeerId(_) | RemoteIdUnavailable | SelfConnect => {
                            tracing::warn!(err = %err, "ingress connections error");
                        },
                        Connect(_) | Endpoint(_) | Io(_) | Shutdown | Signer(_) | Task(_) => {
                            tracing::error!(err = %err, "ingress connections error");
                            break;
                        },
                    },
                },
                None => {
                    break;
                }
            },

            task = tasks.next() => {
                if let Some(Err(e)) = task {
                    drop(e.into_cancelled())
                }
            }
        }
    }
    tracing::debug!("ingress connections done, draining tasks");
    while let Some(res) = tasks.next().await {
        if let Err(e) = res {
            drop(e.into_cancelled())
        }
    }
    tracing::debug!("tasks drained");

    Err(quic::Error::Shutdown)
}

struct Maybe<T>(Option<T>);

impl<T> Maybe<T> {
    async fn or_else<F, Fut>(self, f: F) -> Maybe<T>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Option<T>>,
    {
        match self.0 {
            None => Self(f().await),
            Some(x) => Self(Some(x)),
        }
    }

    fn into_inner(self) -> Option<T> {
        self.0
    }
}

pub(in crate::net::protocol) async fn get_or_connect<S, Addrs>(
    state: &State<S>,
    remote_id: PeerId,
    addrs: Addrs,
) -> Option<quic::Connection>
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
    Addrs: IntoIterator<Item = SocketAddr> + 'static,
{
    Maybe(state.endpoint.get_connection(remote_id))
        .or_else(|| connect_accept(state.clone(), remote_id, addrs))
        .await
        .into_inner()
}

pub(in crate::net::protocol) async fn connect_accept<S, Addrs>(
    state: State<S>,
    remote_id: PeerId,
    addrs: Addrs,
) -> Option<quic::Connection>
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
    Addrs: IntoIterator<Item = SocketAddr> + 'static,
{
    connect(&state.endpoint, remote_id, addrs)
        .await
        .map(|(conn, ingress)| {
            let spawner = state.spawner.clone();
            spawner
                .spawn(super::streams::incoming(state, ingress))
                .detach();
            conn
        })
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
