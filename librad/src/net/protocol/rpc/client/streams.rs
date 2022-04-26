// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use either::Either;
use futures::StreamExt as _;

use crate::{
    net::{
        connection::{CloseReason, RemoteAddr as _, RemotePeer},
        protocol::io,
        quic,
        upgrade,
    },
    paths::Paths,
};

use super::error;

/// Dispatch a stream of bidirectional, git streams.
///
/// This will deny all other streams.
///
/// # Panics
///
/// Panics if one of the tasks spawned by this function panics.
#[tracing::instrument(
    skip(incoming),
    fields(
        remote_id = %incoming.remote_peer_id(),
        remote_addr = %incoming.remote_addr()
    )
)]
pub(super) async fn git(
    paths: Arc<Paths>,
    mut incoming: quic::BoxedIncomingStreams<'static>,
) -> Result<(), error::Incoming> {
    use Either::{Left, Right};

    while let Some(stream) = incoming.next().await {
        match stream? {
            Left(bidi) => incoming::bidi(paths.clone(), bidi).await,
            Right(uni) => {
                incoming::deny_uni(uni);
                return Err(error::Incoming::Uni);
            },
        }
    }

    Ok(())
}

mod incoming {
    use super::*;

    pub(super) async fn bidi(paths: Arc<Paths>, stream: quic::BidiStream) {
        use upgrade::SomeUpgraded::*;

        match upgrade::with_upgraded(stream).await {
            Err(upgrade::Error { stream, source }) => {
                tracing::warn!(err = ?source, "invalid upgrade");
                stream.close(CloseReason::InvalidUpgrade)
            },

            Ok(Git(up)) => io::recv::git(&paths, up).await,
            Ok(Gossip(up)) => deny_bidi(up.into_stream(), "gossip"),
            Ok(Membership(up)) => deny_bidi(up.into_stream(), "membership"),
            Ok(Interrogation(up)) => deny_bidi(up.into_stream(), "interrogation"),
            Ok(RequestPull(up)) => deny_bidi(up.into_stream(), "request-pull"),
        }
    }

    pub(super) fn deny_uni(stream: quic::RecvStream) {
        tracing::warn!("unidirectional requested");
        stream.close(CloseReason::InvalidUpgrade)
    }

    fn deny_bidi(stream: quic::BidiStream, kind: &str) {
        tracing::warn!("non-git bidirectional {} requested", kind);
        stream.close(CloseReason::InvalidUpgrade)
    }
}
