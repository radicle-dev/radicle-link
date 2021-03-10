// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::BTreeSet, net::SocketAddr, panic};

use futures::{
    future::{self, TryFutureExt as _},
    stream::{FuturesUnordered, StreamExt as _},
};

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
/// Panics if one of the tasks [`tokio::spawn`]ed by this function panics.
#[tracing::instrument(skip(state, ingress), err)]
pub(in crate::net::protocol) async fn incoming<S, I>(
    state: State<S>,
    ingress: I,
) -> Result<!, quic::Error>
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
    I: futures::Stream<Item = quic::Result<(quic::Connection, quic::IncomingStreams<'static>)>>,
{
    use quic::Error::*;

    let listen_addrs = state.endpoint.listen_addrs()?;
    state.phone.emit(event::Endpoint::Up { listen_addrs });

    let mut tasks = FuturesUnordered::new();
    let ingress = ingress.fuse();
    futures::pin_mut!(ingress);
    loop {
        futures::select! {
            conn = ingress.next() => match conn {
                Some(conn) => match conn {
                    Ok((_, streams)) => {
                        tasks.push(tokio::spawn(streams::incoming(state.clone(), streams)));
                    },
                    Err(err)=> match err {
                        Connection(_) | PeerId(_) | RemoteIdUnavailable | SelfConnect => {
                            tracing::warn!(err = %err, "ingress connections error");
                        },
                        Connect(_) | Endpoint(_) | Io(_) | Shutdown | Signer(_) => {
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
                    if let Ok(panik) = e.try_into_panic() {
                        panic::resume_unwind(panik)
                    }
                }
            }
        }
    }
    tracing::debug!("ingress connections done, draining tasks");
    while let Some(res) = tasks.next().await {
        if let Err(e) = res {
            if let Ok(panik) = e.try_into_panic() {
                panic::resume_unwind(panik)
            }
        }
    }
    tracing::debug!("tasks drained");

    Err(quic::Error::Shutdown)
}

pub async fn connect_peer_info<'a>(
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
pub async fn connect<'a, Addrs>(
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
