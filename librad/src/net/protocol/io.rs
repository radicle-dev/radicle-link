// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{iter, net::SocketAddr};

use data::BoundedVec;
use futures::stream::{self, StreamExt as _};
use tracing::Instrument as _;

use super::{
    gossip,
    info::{PartialPeerInfo, PeerAdvertisement},
    membership,
    tick,
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

pub(super) mod graft;
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
            state.membership.hello(peer_advertisement(&state.endpoint)),
        )
        .await;

        match rpc_sent {
            Err(e) => tracing::warn!(err = ?e, "failed to send membership hello"),
            Ok(()) => {
                let info = || peer_advertisement(&state.endpoint);
                let membership::TnT { trans, ticks } =
                    state.membership.connection_established(PartialPeerInfo {
                        peer_id: peer,
                        advertised_info: None,
                        seen_addrs: BoundedVec::singleton(conn.remote_addr()),
                    });

                trans.into_iter().for_each(|evt| state.phone.emit(evt));
                for tick in ticks {
                    stream::iter(membership::collect_tocks(&state.membership, &info, tick))
                        .for_each(|tock| tick::tock(state.clone(), tock))
                        .await
                }

                tokio::spawn(streams::incoming(state.clone(), ingress).in_current_span());
            },
        }
    }
}

pub(super) fn peer_advertisement(endpoint: &quic::Endpoint) -> PeerAdvertisement<SocketAddr> {
    let mut listen_addrs = BoundedVec::from(iter::empty());
    listen_addrs.extend_fill(endpoint.listen_addrs());
    PeerAdvertisement {
        listen_addrs,
        capabilities: Default::default(),
    }
}
