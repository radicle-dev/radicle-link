// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{net::SocketAddr, panic};

use futures::stream::{FuturesUnordered, StreamExt as _, TryStreamExt as _};

use crate::net::{
    connection::{CloseReason, Duplex as _, RemoteAddr as _, RemotePeer},
    protocol::{gossip, ProtocolStorage, State},
    quic,
    upgrade,
};

/// Dispatch incoming streams.
///
/// # Panics
///
/// Panics if one of the tasks [`tokio::spawn`]ed by this function panics.
#[tracing::instrument(skip(state, bidi, uni))]
pub(in crate::net::protocol) async fn incoming<S>(
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

    let mut tasks = FuturesUnordered::new();
    loop {
        futures::select! {
            stream = bidi.next() => match stream {
                Some(item) => match item {
                    Ok(stream) => tasks.push(tokio::spawn(incoming::bidi(state.clone(), stream))),
                    Err(e) => {
                        tracing::warn!(err = ?e, "ingress bidi error");
                        break;
                    }
                },
                None => {
                    break;
                }
            },
            stream = uni.next() => match stream {
                Some(item) => match item {
                    Ok(stream) => tasks.push(tokio::spawn(incoming::uni(state.clone(), stream))),
                    Err(e) => {
                        tracing::warn!(err = ?e, "ingress uni error");
                        break;
                    }
                },
                None => {
                    break;
                }
            },
            res = tasks.next() => {
                if let Some(Err(e)) = res {
                    if let Ok(panik) = e.try_into_panic() {
                        panic::resume_unwind(panik)
                    }
                }
            },
            complete => {
                break;
            }
        }
    }
    tracing::debug!("ingress streams done, draining tasks");
    while let Some(res) = tasks.next().await {
        if let Err(e) = res {
            if let Ok(panik) = e.try_into_panic() {
                panic::resume_unwind(panik)
            }
        }
    }
    tracing::debug!("tasks drained");
}

mod incoming {
    use super::*;

    use crate::net::protocol::io::recv;

    pub(super) async fn bidi<S>(state: State<S>, stream: quic::BidiStream)
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

            Ok(Gossip(up)) => recv::gossip(state, up).await,
            Ok(Membership(up)) => recv::membership(state, up).await,
        }
    }

    pub(super) async fn uni<S>(state: State<S>, stream: quic::RecvStream)
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

            Ok(Gossip(up)) => recv::gossip(state, up).await,
            Ok(Membership(up)) => recv::membership(state, up).await,
        }
    }
}
