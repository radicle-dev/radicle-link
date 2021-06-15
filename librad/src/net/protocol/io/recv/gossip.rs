// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{iter, net::SocketAddr};

use futures::{
    io::{AsyncRead, BufReader},
    stream::{self, StreamExt as _},
};
use futures_codec::FramedRead;

use crate::{
    net::{
        connection::{RemoteAddr, RemotePeer},
        protocol::{
            broadcast,
            event,
            gossip,
            info::PeerInfo,
            io::{codec, peer_advertisement},
            membership,
            tick,
            ProtocolStorage,
            State,
        },
        upgrade::{self, Upgraded},
    },
    PeerId,
};

pub(in crate::net::protocol) async fn gossip<S, T>(
    state: State<S>,
    stream: Upgraded<upgrade::Gossip, T>,
) where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
    T: RemotePeer + RemoteAddr<Addr = SocketAddr> + AsyncRead + Unpin,
{
    let remote_id = stream.remote_peer_id();

    let mut recv = FramedRead::new(
        BufReader::with_capacity(100, stream.into_stream()),
        codec::Gossip::new(),
    );

    while let Some(x) = recv.next().await {
        match x {
            Err(e) => {
                tracing::warn!(err = ?e, "gossip recv error");
                let membership::TnT { trans, ticks } = state.membership.connection_lost(remote_id);
                let tocks = membership_tocks(&state, ticks.into_iter());
                eval_events(&state, trans);
                eval_tocks(state.clone(), tocks).await;

                break;
            },

            Ok(msg) => {
                let peer_info = || PeerInfo {
                    peer_id: state.local_id,
                    advertised_info: peer_advertisement(&state.endpoint),
                    seen_addrs: iter::empty().into(),
                };
                state
                    .phone
                    .emit_diagnostic_event(event::NetworkDiagnosticEvent::gossip_received(
                        recv.remote_addr(),
                        msg.clone(),
                    ));
                match broadcast::apply(
                    &state.membership,
                    &state.storage,
                    &peer_info,
                    remote_id,
                    msg,
                )
                .await
                {
                    // Partial view states diverge apparently, and the stream is
                    // (assumed to be) unidirectional. Thus, send a DISCONNECT
                    // to sync states.
                    Err(broadcast::Error::Unsolicited { remote_id, .. }) => {
                        tracing::warn!(
                            remote_id = %remote_id,
                            "unsolicited broadcast message, sending disconnect"
                        );
                        let tocks =
                            membership_tocks(&state, Some(disconnect(remote_id)).into_iter());
                        eval_tocks(state.clone(), tocks).await;

                        break;
                    },

                    Ok((may_event, tocks)) => {
                        eval_events(&state, may_event);
                        eval_tocks(state.clone(), tocks).await
                    },
                }
            },
        }
    }
}

fn membership_tocks<'a, S, I>(
    state: &'a State<S>,
    ticks: I,
) -> impl Iterator<Item = tick::Tock<SocketAddr, gossip::Payload>> + 'a
where
    I: Iterator<Item = membership::Tick<SocketAddr>> + 'a,
{
    let info = {
        let endpoint = state.endpoint.clone();
        move || peer_advertisement(&endpoint)
    };
    ticks.flat_map(move |tick| membership::collect_tocks(&state.membership, &info, tick))
}

async fn eval_tocks<S, I>(state: State<S>, tocks: I)
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
    I: IntoIterator<Item = tick::Tock<SocketAddr, gossip::Payload>>,
{
    stream::iter(tocks)
        .for_each(|tock| tick::tock(state.clone(), tock))
        .await
}

fn eval_events<S, I, E>(state: &State<S>, evts: I)
where
    I: IntoIterator<Item = E>,
    E: Into<event::Upstream>,
{
    for evt in evts {
        state.phone.emit(evt)
    }
}

fn disconnect<A>(remote_id: PeerId) -> membership::Tick<A> {
    membership::Tick::Reply {
        to: remote_id,
        message: membership::Message::Disconnect,
    }
}
