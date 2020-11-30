// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::BTreeSet, iter, net::SocketAddr};

use futures::{
    future::{self, TryFutureExt as _},
    io::AsyncWrite,
};

use super::Error;
use crate::{
    net::{
        connection::{Closable, CloseReason},
        gossip,
        quic,
        upgrade::{self, UpgradeRequest, Upgraded},
    },
    peer::PeerId,
};

pub async fn connect_peer_info<'a>(
    endpoint: &quic::Endpoint,
    peer_info: gossip::PeerInfo<SocketAddr>,
) -> Option<(quic::Connection, quic::IncomingStreams<'a>)> {
    let advertised_port = peer_info.advertised_info.listen_addr.port();
    let addrs = iter::once(peer_info.advertised_info.listen_addr).chain(
        peer_info
            .seen_addrs
            .into_iter()
            .flat_map(|addr| vec![addr, SocketAddr::new(addr.ip(), advertised_port)]),
    );
    connect(endpoint, peer_info.peer_id, addrs).await
}

pub async fn connect<'a, Addrs>(
    endpoint: &quic::Endpoint,
    peer_id: PeerId,
    addrs: Addrs,
) -> Option<(quic::Connection, quic::IncomingStreams<'a>)>
where
    Addrs: IntoIterator<Item = SocketAddr>,
{
    fn routable(addr: &SocketAddr) -> bool {
        let ip = addr.ip();
        !(ip.is_unspecified() || ip.is_documentation() || ip.is_multicast())
    }

    let addrs = addrs.into_iter().filter(routable).collect::<BTreeSet<_>>();
    if addrs.is_empty() {
        tracing::warn!("no routable addrs for {}", peer_id);
        None
    } else {
        future::select_ok(addrs.iter().map(|addr| {
            let mut endpoint = endpoint.clone();
            tracing::info!(remote.id = %peer_id, remote.addr = %addr, "establishing connection");
            Box::pin(async move {
                endpoint
                    .connect(peer_id, &addr)
                    .map_err(|e| {
                        tracing::warn!("could not connect to {} at {}: {}", peer_id, addr, e);
                        e
                    })
                    .await
            })
        }))
        .await
        .ok()
        .map(|(success, _pending)| success)
    }
}

pub async fn upgrade_stream<S, U>(stream: S, up: U) -> Result<Upgraded<U, S>, Error>
where
    S: Closable + AsyncWrite + Unpin + Send + Sync,
    U: Into<UpgradeRequest>,
{
    upgrade::upgrade(stream, up)
        .await
        .map_err(|upgrade::Error { stream, source }| {
            stream.close(CloseReason::InvalidUpgrade);
            Error::from(source)
        })
}
