// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{iter, net::SocketAddr};

use futures::{
    io::{AsyncRead, BufReader},
    stream::StreamExt as _,
};
use futures_codec::FramedRead;

use crate::{
    net::{
        connection::RemotePeer,
        protocol::{
            broadcast,
            gossip,
            info::PeerInfo,
            io::{codec, peer_advertisement},
            membership,
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
    T: RemotePeer + AsyncRead + Unpin,
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
                state.emit(trans);
                state
                    .tick(membership::tocks(
                        &state.membership,
                        peer_advertisement(&state.endpoint),
                        ticks,
                    ))
                    .await;

                break;
            },

            Ok(msg) => {
                let peer_info = || PeerInfo {
                    peer_id: state.local_id,
                    advertised_info: peer_advertisement(&state.endpoint)(),
                    seen_addrs: iter::empty().into(),
                };
                match broadcast::apply(&state.membership, &state.storage, peer_info, remote_id, msg)
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
                        state
                            .tick(membership::tocks(
                                &state.membership,
                                peer_advertisement(&state.endpoint),
                                Some(disconnect(remote_id)),
                            ))
                            .await;

                        break;
                    },

                    Ok((may_event, tocks)) => {
                        state.emit(may_event);
                        state.tick(tocks).await;
                    },
                }
            },
        }
    }
}

fn disconnect<A>(remote_id: PeerId) -> membership::Tick<A> {
    membership::Tick::Reply {
        to: remote_id,
        message: membership::Message::Disconnect,
    }
}
