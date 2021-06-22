// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{iter, net::SocketAddr};

use data::BoundedVec;

use super::{
    gossip,
    info::{PartialPeerInfo, PeerAdvertisement},
    membership,
    ProtocolStorage,
    State,
};
use crate::{
    net::{connection::RemoteAddr as _, quic},
    PeerId,
};

mod codec;

pub(super) mod connections;
pub(super) use connections::{connect, connect_peer_info};

pub mod error;

pub mod graft;
pub(super) mod recv;

pub mod send;
pub use send::{rpc::Rpc, send_rpc};

pub(super) mod streams;

#[tracing::instrument(skip(state, peer, addrs), fields(remote_id = %peer))]
pub(super) async fn discovered<S>(state: State<S>, peer: PeerId, addrs: Vec<SocketAddr>)
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
{
    if state.endpoint.get_connection(peer).is_some() {
        return;
    }

    if let Some((conn, ingress)) = connect(&state.endpoint, peer, addrs).await {
        let rpc_sent = send_rpc::<_, ()>(
            &conn,
            state
                .membership
                .hello(peer_advertisement(&state.endpoint)()),
        )
        .await;

        match rpc_sent {
            Err(e) => tracing::warn!(err = ?e, "failed to send membership hello"),
            Ok(()) => {
                let membership::TnT { trans, ticks } =
                    state.membership.connection_established(PartialPeerInfo {
                        peer_id: peer,
                        advertised_info: None,
                        seen_addrs: BoundedVec::singleton(conn.remote_addr()),
                    });

                state.emit(trans);
                state
                    .tick(membership::tocks(
                        &state.membership,
                        peer_advertisement(&state.endpoint),
                        ticks,
                    ))
                    .await;
                state
                    .spawner
                    .spawn(streams::incoming(state.clone(), ingress))
                    .detach();
            },
        }
    }
}

pub(super) fn peer_advertisement(
    endpoint: &quic::Endpoint,
) -> impl Fn() -> PeerAdvertisement<SocketAddr> + '_ {
    move || {
        let mut listen_addrs = BoundedVec::from(iter::empty());
        listen_addrs.extend_fill(endpoint.listen_addrs());
        PeerAdvertisement {
            listen_addrs,
            capabilities: Default::default(),
        }
    }
}
