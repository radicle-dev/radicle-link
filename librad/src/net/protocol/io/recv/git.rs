// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{io, net::SocketAddr, panic};

use futures::{
    future::TryFutureExt as _,
    io::{AsyncRead, AsyncWrite},
    stream::{FuturesUnordered, StreamExt as _},
};
use thiserror::Error;
use tracing::Instrument as _;

use crate::{
    git::{p2p::header::Header, Urn},
    net::{
        connection::{Duplex, RemoteInfo},
        protocol::{gossip, io::graft, ProtocolStorage, State},
        upgrade::{self, Upgraded},
    },
    PeerId,
};

#[derive(Debug, Error)]
enum Error {
    #[error(transparent)]
    Rere(#[from] graft::error::Rere),

    #[error(transparent)]
    Io(#[from] io::Error),
}

pub(in crate::net::protocol) async fn git<S, T>(state: State<S>, stream: Upgraded<upgrade::Git, T>)
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
    T: Duplex + RemoteInfo<Addr = SocketAddr>,
    <T as Duplex>::Read: AsyncRead + Send + Sync + Unpin + 'static,
    <T as Duplex>::Write: AsyncWrite + Send + Sync + Unpin + 'static,
{
    let remote_peer = stream.remote_peer_id();
    let remote_addr = stream.remote_addr();
    let (recv, send) = stream.into_stream().split();
    match state.git.service(recv, send).await {
        Err(e) => tracing::warn!(err = ?e, "git service setup error"),
        Ok(srv) => {
            let tasks = FuturesUnordered::new();
            let Header { repo, nonce, .. } = &srv.header;
            // Only rere if we have a fresh nonce
            if let Some(n) = nonce {
                if !state.nonces.contains(n) {
                    tasks.push(spawn_rere(&state, repo.clone(), remote_peer, remote_addr))
                }
                state.nonces.insert(*n)
            }
            tasks.push(tokio::spawn(
                srv.run().err_into::<Error>().in_current_span(),
            ));

            let results = tasks.take(2).collect::<Vec<_>>().await;
            for res in results {
                match res {
                    Err(e) => {
                        if e.is_panic() {
                            panic::resume_unwind(e.into_panic())
                        } else if e.is_cancelled() {
                            tracing::info!("cancelled task")
                        } else {
                            unreachable!("unexpected task error: {:?}", e)
                        }
                    },

                    Ok(Ok(())) => tracing::debug!("task done"),
                    Ok(Err(e)) => tracing::warn!(err = ?e, "task error"),
                }
            }
        },
    }
}

fn spawn_rere<S>(
    state: &State<S>,
    urn: Urn,
    remote_peer: PeerId,
    remote_addr: SocketAddr,
) -> tokio::task::JoinHandle<Result<(), Error>>
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
{
    tokio::spawn({
        let pool = state.storage.clone();
        let config = graft::config::Rere {
            replication: state.config.replication,
            fetch_slot_wait_timeout: state.config.fetch.fetch_slot_wait_timeout,
        };
        let span = tracing::info_span!("rere", urn = %urn, remote_peer = %remote_peer);
        async move {
            tracing::info!("attempting rere");
            let span = tracing::info_span!("rere", urn = %urn, remote_peer = %remote_peer);
            graft::rere(pool, config, urn, remote_peer, Some(remote_addr))
                .instrument(span)
                .await
                .map_err(Error::from)
                .map(|res| match res {
                    None => tracing::info!("rere didn't fetch"),
                    Some(xs) => tracing::info!("rere fetched {} refs", xs.updated_tips.len()),
                })
        }
        .instrument(span)
    })
}
