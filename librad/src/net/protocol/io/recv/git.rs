// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{io, net::SocketAddr};

use futures::{
    future::{self, TryFutureExt as _},
    io::{AsyncRead, AsyncWrite},
    stream::{FuturesUnordered, StreamExt as _},
};
use thiserror::Error;

use crate::{
    git::{replication::ReplicateResult, Urn},
    net::{
        connection::{Duplex, RemoteInfo},
        protocol::{self, control, gossip, io::graft, ProtocolStorage, State},
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
            let repo = srv.header.repo.clone();
            let nonce = srv.header.nonce;
            let res = srv
                .run()
                .err_into::<Error>()
                .and_then(|()| async {
                    if let Some(n) = nonce {
                        // Only rere if grafting is enabled and the nonce is fresh
                        if state.is_graft_enabled() && !state.nonces.contains(&n) {
                            return rere(state.clone(), repo, remote_peer, remote_addr).await;
                        }
                        tracing::warn!(
                            "skipping rere: {}, {}",
                            state.is_graft_enabled(),
                            state.nonces.contains(&n)
                        )
                    }

                    Ok(())
                })
                .await;

            if let Some(n) = nonce {
                state.nonces.insert(n);
            }

            if let Err(e) = res {
                tracing::warn!(err = ?e, "recv git error")
            }
        },
    }
}

#[tracing::instrument(
    skip(state, urn, remote_peer, remote_addr),
    fields(urn = %urn, remote_peer = %remote_peer)
)]
async fn rere<S>(
    state: State<S>,
    urn: Urn,
    remote_peer: PeerId,
    remote_addr: SocketAddr,
) -> Result<(), Error>
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
{
    use protocol::event::downstream::Gossip::Announce;

    tracing::info!("attempting rere");

    let config = graft::Config {
        replication: state.config.replication,
        fetch_slot_wait_timeout: state.config.fetch.fetch_slot_wait_timeout,
    };
    let updated_tips = graft::rere(
        &state.spawner,
        &state.storage,
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
        Some(xs) => {
            tracing::info!("rere updated {} refs", xs.len());
            if !xs.is_empty() {
                tracing::trace!("refs updated by rere: {:?}", xs);
            }
            xs.into_iter()
                .map(|(refl, head)| {
                    control::gossip(
                        &state,
                        Announce(gossip::Payload {
                            urn: urn.clone(),
                            rev: Some(head.into()),
                            origin: refl
                                .split('/')
                                .skip_while(|&x| x != "remotes")
                                .skip(1)
                                .take(1)
                                .next()
                                .and_then(|remote| remote.parse().ok()),
                        }),
                        Some(remote_peer),
                    )
                })
                .collect::<FuturesUnordered<_>>()
                .for_each(future::ready)
                .await
        },
    }

    Ok(())
}
