// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::net::SocketAddr;

use either::Either;
use futures::stream::{Stream, StreamExt as _};

use super::recv;
use crate::net::{
    connection::{CloseReason, RemoteAddr as _, RemotePeer},
    protocol::{gossip, ProtocolStorage, RequestPullGuard, State},
    quic,
    upgrade,
};

/// Dispatch incoming streams.
///
/// # Panics
///
/// Panics if one of the tasks spawned by this function panics.
#[tracing::instrument(
    skip(state, streams),
    fields(
        remote_id = %streams.remote_peer_id(),
        remote_addr = %streams.remote_addr()
    )
)]
pub(in crate::net::protocol) async fn incoming<S, G, I>(
    state: State<S, G>,
    streams: quic::IncomingStreams<I>,
) where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
    G: RequestPullGuard,
    I: Stream<Item = quic::Result<Either<quic::BidiStream, quic::RecvStream>>> + Unpin,
{
    use Either::{Left, Right};

    let remote_id = streams.remote_peer_id();

    let streams = streams.fuse();
    futures::pin_mut!(streams);
    loop {
        match streams.next().await {
            None => {
                recv::connection_lost(state, remote_id).await;
                break;
            },
            Some(stream) => {
                tracing::info!("new ingress stream");
                match stream {
                    Ok(s) => match s {
                        Left(bidi) => state
                            .spawner
                            .spawn(incoming::bidi(state.clone(), bidi))
                            .detach(),
                        Right(uni) => state
                            .spawner
                            .spawn(incoming::uni(state.clone(), uni))
                            .detach(),
                    },
                    Err(e) => {
                        tracing::warn!(err = ?e, "ingress stream error");
                        recv::connection_lost(state, remote_id).await;
                        break;
                    },
                }
            },
        }
    }
}

mod incoming {
    use super::*;

    use crate::net::protocol::io::recv;

    pub(super) async fn bidi<S, G>(state: State<S, G>, stream: quic::BidiStream)
    where
        S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
        G: RequestPullGuard,
    {
        use upgrade::SomeUpgraded::*;

        match upgrade::with_upgraded(stream).await {
            Err(upgrade::Error { stream, source }) => {
                tracing::warn!(err = ?source, "invalid upgrade");
                stream.close(CloseReason::InvalidUpgrade)
            },

            Ok(Git(up)) => recv::git(&state.config.paths, up).await,
            Ok(Gossip(up)) => recv::gossip(state, up).await,
            Ok(Membership(up)) => recv::membership(state, up).await,
            Ok(Interrogation(up)) => recv::interrogation(state, up).await,
            Ok(RequestPull(up)) => recv::request_pull(state, up).await,
        }
    }

    pub(super) async fn uni<S, G>(state: State<S, G>, stream: quic::RecvStream)
    where
        S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
        G: RequestPullGuard,
    {
        use upgrade::SomeUpgraded::*;

        match upgrade::with_upgraded(stream).await {
            Err(upgrade::Error { stream, source }) => {
                tracing::warn!(err = ?source, "invalid upgrade");
                stream.close(CloseReason::InvalidUpgrade)
            },

            Ok(Git(up)) => deny_uni(up.into_stream(), "git"),
            Ok(Interrogation(up)) => deny_uni(up.into_stream(), "interrogation"),
            Ok(RequestPull(up)) => deny_uni(up.into_stream(), "request-pull"),

            Ok(Gossip(up)) => recv::gossip(state, up).await,
            Ok(Membership(up)) => recv::membership(state, up).await,
        }
    }

    fn deny_uni(stream: quic::RecvStream, kind: &str) {
        tracing::warn!("unidirectional {} requested", kind);
        stream.close(CloseReason::InvalidUpgrade)
    }
}
