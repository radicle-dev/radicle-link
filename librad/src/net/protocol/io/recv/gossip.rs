// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::net::SocketAddr;

use futures::{
    io::AsyncRead,
    stream::{self, StreamExt as _},
};
use futures_codec::FramedRead;

use crate::net::{
    connection::RemotePeer,
    protocol::{
        broadcast,
        gossip,
        info::PeerInfo,
        io::{codec, peer_advertisement},
        membership,
        tick,
        ProtocolStorage,
        State,
    },
    upgrade::{self, Upgraded},
};

pub(in crate::net::protocol) async fn gossip<S, T>(
    state: State<S>,
    stream: Upgraded<upgrade::Gossip, T>,
) where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
    T: RemotePeer + AsyncRead + Unpin,
{
    let mut recv = FramedRead::new(stream.into_stream(), codec::Gossip::new());
    let remote_id = recv.remote_peer_id();

    while let Some(x) = recv.next().await {
        match x {
            Err(e) => {
                tracing::warn!(err = ?e, "gossip recv error");
                let info = || peer_advertisement(&state.endpoint);

                let membership::TnT { trans, ticks } = state.membership.connection_lost(remote_id);
                trans.into_iter().for_each(|evt| state.phone.emit(evt));
                for tick in ticks {
                    stream::iter(membership::collect_tocks(&state.membership, &info, tick))
                        .for_each(|tock| tick::tock(state.clone(), tock))
                        .await
                }

                break;
            },

            Ok(msg) => {
                let peer_info = || PeerInfo {
                    peer_id: state.local_id,
                    advertised_info: peer_advertisement(&state.endpoint),
                    seen_addrs: Default::default(),
                };
                match broadcast::apply(
                    &state.membership,
                    &state.storage,
                    &peer_info,
                    remote_id,
                    msg,
                )
                .await
                {
                    Err(e) => {
                        tracing::warn!(err = ?e, "gossip error");
                        break;
                    },

                    Ok((may_event, tocks)) => {
                        if let Some(event) = may_event {
                            state.phone.emit(event)
                        }

                        stream::iter(tocks)
                            .for_each(|tock| tick::tock(state.clone(), tock))
                            .await
                    },
                }
            },
        }
    }
}
