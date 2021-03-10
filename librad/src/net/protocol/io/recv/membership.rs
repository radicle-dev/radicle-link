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
    connection::RemoteInfo,
    protocol::{
        gossip,
        io::{codec, peer_advertisement},
        membership,
        tick,
        ProtocolStorage,
        State,
    },
    upgrade::{self, Upgraded},
};

pub(in crate::net::protocol) async fn membership<S, T>(
    state: State<S>,
    stream: Upgraded<upgrade::Membership, T>,
) where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
    T: RemoteInfo<Addr = SocketAddr> + AsyncRead + Unpin,
{
    let mut recv = FramedRead::new(stream.into_stream(), codec::Membership::new());
    let remote_id = recv.remote_peer_id();
    let remote_addr = recv.remote_addr();

    while let Some(x) = recv.next().await {
        match x {
            Err(e) => {
                tracing::warn!(err = ?e, "membership recv error");
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
                let info = || peer_advertisement(&state.endpoint);
                match membership::apply(&state.membership, &info, remote_id, remote_addr, msg) {
                    Err(e) => {
                        tracing::warn!(err = ?e, "membership error");
                        break;
                    },

                    Ok((trans, tocks)) => {
                        trans.into_iter().for_each(|evt| state.phone.emit(evt));
                        stream::iter(tocks)
                            .for_each(|tock| tick::tock(state.clone(), tock))
                            .await
                    },
                }
            },
        }
    }
}
