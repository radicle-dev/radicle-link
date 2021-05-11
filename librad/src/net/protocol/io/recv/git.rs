// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{io, net::SocketAddr};

use futures::{
    future::TryFutureExt as _,
    io::{AsyncRead, AsyncWrite},
    stream::{FuturesUnordered, StreamExt as _},
};
use thiserror::Error;
use tracing::Instrument as _;

use crate::{
    executor,
    git::{p2p::header::Header, replication::ReplicateResult, Urn},
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
                    tasks.push(spawn_rere(
                        state.clone(),
                        repo.clone(),
                        remote_peer,
                        remote_addr,
                    ))
                }
                state.nonces.insert(*n)
            }
            tasks.push(state.spawner.spawn(srv.run().err_into::<Error>()));

            let results = tasks.take(2).collect::<Vec<_>>().await;
            for res in results {
                match res {
                    Err(e) => {
                        let _ = e.into_cancelled();
                        tracing::info!("cancelled task")
                    },
                    Ok(Ok(())) => tracing::debug!("task done"),
                    Ok(Err(e)) => tracing::warn!(err = ?e, "task error"),
                }
            }
        },
    }
}

fn spawn_rere<S>(
    state: State<S>,
    urn: Urn,
    remote_peer: PeerId,
    remote_addr: SocketAddr,
) -> executor::JoinHandle<Result<(), Error>>
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
{
    let spawner = state.spawner.clone();
    spawner.spawn({
        let spawner = spawner.clone();
        let pool = state.storage.clone();
        let config = graft::config::Rere {
            replication: state.config.replication,
            fetch_slot_wait_timeout: state.config.fetch.fetch_slot_wait_timeout,
        };
        let span = tracing::info_span!("rere", urn = %urn, remote_peer = %remote_peer);
        async move {
            tracing::info!("attempting rere");
            let updated_tips = graft::rere(
                &spawner,
                &pool,
                config,
                urn.clone(),
                remote_peer,
                Some(remote_addr),
            )
            .await
            .map_err(Error::from)?
            .map(|ReplicateResult { updated_tips, .. }| updated_tips);
            match updated_tips {
                None => tracing::info!("rere skipped"),
                Some(xs) => tracing::info!("rere updated {} refs", xs.len()),
            }

            Ok(())
        }
        .instrument(span)
    })
}
