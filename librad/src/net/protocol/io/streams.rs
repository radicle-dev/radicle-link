// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{net::SocketAddr, panic};

use either::Either;
use futures::stream::{FuturesUnordered, Stream, StreamExt as _};

use super::recv;
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
#[tracing::instrument(
    skip(state, streams),
    fields(
        remote_id = %streams.remote_peer_id(),
        remote_addr = %streams.remote_addr()
    )
)]
pub(in crate::net::protocol) async fn incoming<S, I>(
    state: State<S>,
    streams: quic::IncomingStreams<I>,
) where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
    I: Stream<Item = quic::Result<Either<quic::BidiStream, quic::RecvStream>>> + Unpin,
{
    use Either::{Left, Right};

    let remote_id = streams.remote_peer_id();

    let mut streams = streams.fuse();
    let mut tasks = FuturesUnordered::new();
    loop {
        futures::select! {
            next_stream = streams.next() => match next_stream {
                None => {
                    recv::connection_lost(state, remote_id).await;
                    break;
                },
                Some(stream) => {
                    tracing::info!("new ingress stream");
                    match stream {
                        Ok(s) => {
                            let task = match s {
                                Left(bidi) => tokio::spawn(incoming::bidi(state.clone(), bidi)),
                                Right(uni) => tokio::spawn(incoming::uni(state.clone(), uni)),
                            };
                            tasks.push(task)
                        },
                        Err(e) => {
                            tracing::warn!(err = ?e, "ingress stream error");
                            recv::connection_lost(state, remote_id).await;
                            break;
                        }
                    }
                }
            },

            res = tasks.next() => {
                if let Some(Err(e)) = res {
                    if let Ok(panik) = e.try_into_panic() {
                        panic::resume_unwind(panik)
                    }
                }
            },

            complete => break
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
