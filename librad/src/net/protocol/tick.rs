// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::net::SocketAddr;

use futures::{
    future::{BoxFuture, FutureExt as _, TryFutureExt as _},
    stream::{FuturesOrdered, StreamExt as _},
};

use super::{error, gossip, io, membership, PeerInfo, ProtocolStorage, RequestPullGuard, State};
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

    /// Attempt to connect, send "hello" message (either JOIN or NEIGHBOUR), and
    /// add to active set if that succeeds
    Connect {
        to: PeerInfo<A>,
        message: io::Rpc<A, P>,
    },

    /// Close connections due to eviction from partial view
    Disconnect { peer: PeerId },
}

#[tracing::instrument(level = "debug", skip(state))]
pub(super) async fn tock<S, G>(state: State<S, G>, tock: Tock<SocketAddr, gossip::Payload>)
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + 'static,
    G: RequestPullGuard,
{
    let mut mcfly = FuturesOrdered::new();
    mcfly.push(one_tock(state.clone(), tock));

    while let Some(res) = mcfly.next().await {
        tracing::debug!("tock");
        let cont = res.unwrap_or_else(|e| match e {
            error::Tock::Reliable(error::ReliableSend { cont, source }) => {
                tracing::warn!(err = ?source, "reliable send error");
                cont
            },
            error::Tock::Unreliable(source) => {
                tracing::warn!(err = ?source, "unreliable send error");
                vec![]
            },
        });

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
    }
}

fn one_tock<S, G>(
    state: State<S, G>,
    tock: Tock<SocketAddr, gossip::Payload>,
) -> BoxFuture<'static, Result<Vec<membership::Tick<SocketAddr>>, error::Tock<SocketAddr>>>
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + 'static,
    G: RequestPullGuard,
{
    use Tock::*;

    async move {
        let mut events = vec![];
        let res = match tock {
            SendConnected { to, message } => match state.connection(to, None).await {
                None => {
                    let membership::TnT { trans, ticks: cont } =
                        state.membership.connection_lost(to);
                    events = trans;
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
                            events = trans;
                            error::Tock::Reliable(error::ReliableSend {
                                cont,
                                source: e.into(),
                            })
                        })
                        .await?;
                    Ok(vec![])
                },
            },

            AttemptSend { to, message } => {
                try_connect_and_send(&state, &to, message).await?;
                Ok(vec![])
            },

            Connect { to, message } => {
                try_connect_and_send(&state, &to, message).await?;

                let membership::TnT { trans, ticks: cont } =
                    state.membership.connection_established(to.into());
                events = trans;

                Ok(cont)
            },

            Disconnect { peer } => {
                state.endpoint.disconnect(&peer);
                Ok(vec![])
            },
        };

        state.emit(events);
        res
    }
    .boxed()
}

async fn try_connect_and_send<S, G>(
    state: &State<S, G>,
    to: &PeerInfo<SocketAddr>,
    message: io::Rpc<SocketAddr, gossip::Payload>,
) -> Result<(), error::BestEffortSend<SocketAddr>>
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + 'static,
    G: RequestPullGuard,
{
    let conn = state
        .connection(to.peer_id, to.addrs().copied().collect::<Vec<_>>())
        .await
        .ok_or_else(|| error::BestEffortSend::CouldNotConnect { to: to.clone() })?;
    io::send_rpc(&conn, message)
        .map_err(error::BestEffortSend::SendGossip)
        .await
}
