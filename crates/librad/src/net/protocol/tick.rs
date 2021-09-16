// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::net::SocketAddr;

use futures::{
    future::{BoxFuture, FutureExt as _, TryFutureExt as _},
    stream::{FuturesOrdered, StreamExt as _},
};

use super::{error, gossip, io, membership, PeerInfo, ProtocolStorage, State};
use crate::PeerId;

#[derive(Debug)]
pub(super) enum Tock<A, P> {
    /// Send to connected peer, or notify of connection loss
    SendConnected { to: PeerId, message: io::Rpc<A, P> },

    /// Attempt to connect + send, ignore failure
    AttemptSend {
        to: PeerInfo<A>,
        message: io::Rpc<A, P>,
    },

    /// Close connections due to eviction from partial view
    Disconnect { peer: PeerId },
}

#[tracing::instrument(level = "debug", skip(state))]
pub(super) async fn tock<S>(state: State<S>, tock: Tock<SocketAddr, gossip::Payload>)
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
{
    let mut mcfly = FuturesOrdered::new();
    mcfly.push(one_tock(state.clone(), tock));

    while let Some(res) = mcfly.next().await {
        tracing::debug!("tock");
        if let Err(e) = res {
            match e {
                error::Tock::Reliable(error::ReliableSend { cont, source }) => {
                    tracing::warn!(err = ?source, "reliable send error");
                    for tick in cont {
                        mcfly.extend(
                            membership::tocks(
                                &state.membership,
                                io::peer_advertisement(&state.endpoint),
                                Some(tick),
                            )
                            .into_iter()
                            .map(|tock| one_tock(state.clone(), tock)),
                        )
                    }
                },
                error::Tock::Unreliable(source) => {
                    tracing::warn!(err = ?source, "unreliable send error");
                },
            }
        }
    }
}

fn one_tock<S>(
    state: State<S>,
    tock: Tock<SocketAddr, gossip::Payload>,
) -> BoxFuture<'static, Result<(), error::Tock<SocketAddr>>>
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
{
    use Tock::*;

    async move {
        match tock {
            SendConnected { to, message } => match state.connection(to, None).await {
                None => {
                    let membership::TnT { trans, ticks: cont } =
                        state.membership.connection_lost(to);
                    state.emit(trans);

                    Err(error::Tock::Reliable(error::ReliableSend {
                        cont,
                        source: error::ReliableSendSource::NotConnected { to },
                    }))
                },

                Some(conn) => {
                    io::send_rpc(&conn, message)
                        .map_err(|e| {
                            let membership::TnT { trans, ticks: cont } =
                                state.membership.connection_lost(to);
                            trans.into_iter().for_each(|evt| state.phone.emit(evt));

                            error::Tock::Reliable(error::ReliableSend {
                                cont,
                                source: e.into(),
                            })
                        })
                        .await
                },
            },

            AttemptSend { to, message } => {
                let conn = state
                    .connection(to.peer_id, to.addrs().copied().collect::<Vec<_>>())
                    .await
                    .ok_or(error::BestEffortSend::CouldNotConnect { to })?;
                Ok(io::send_rpc(&conn, message)
                    .await
                    .map_err(error::BestEffortSend::SendGossip)?)
            },

            Disconnect { peer } => {
                state.endpoint.disconnect(&peer);
                Ok(())
            },
        }
    }
    .boxed()
}
