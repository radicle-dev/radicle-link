// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{iter, net::SocketAddr};

use futures::stream::{self, StreamExt as _};

use super::{broadcast, error, event, gossip, io, tick, PeerInfo, ProtocolStorage, State};
use crate::PeerId;

pub(super) async fn gossip<S>(
    state: &State<S>,
    evt: event::downstream::Gossip,
    exclude: Option<PeerId>,
) where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + 'static,
{
    use event::downstream::Gossip;

    let origin = PeerInfo {
        peer_id: state.local_id,
        advertised_info: io::peer_advertisement(&state.endpoint)(),
        seen_addrs: iter::empty().into(),
    };
    // TODO: answer `Want`s from a provider cache
    let rpc = match evt {
        Gossip::Announce(payload) => broadcast::Message::Have {
            origin,
            val: payload,
        },
        Gossip::Query(payload) => broadcast::Message::Want {
            origin,
            val: payload,
        },
    };
    stream::iter(
        state
            .membership
            .broadcast_recipients(exclude)
            .into_iter()
            .map(|to| tick::Tock::SendConnected {
                to,
                message: rpc.clone().into(),
            }),
    )
    .for_each(|tock| tick::tock(state.clone(), tock))
    .await
}

pub(super) fn info<S>(state: &State<S>, evt: event::downstream::Info)
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + 'static,
{
    use event::downstream::{CacheStats, Info, MembershipInfo, Stats};

    match evt {
        Info::ConnectedPeers(reply) => {
            let chan = reply.lock().take();
            if let Some(tx) = chan {
                tx.send(state.endpoint.peers()).ok();
            }
        },

        Info::Membership(reply) => {
            let chan = reply.lock().take();
            if let Some(tx) = chan {
                tx.send(MembershipInfo {
                    active: state.membership.active(),
                    passive: state.membership.passive(),
                })
                .ok();
            }
        },

        Info::Stats(reply) => {
            let chan = reply.lock().take();
            if let Some(tx) = chan {
                let (active, passive) = state.membership.view_stats();
                tx.send(Stats {
                    connections_total: state.endpoint.connections_total(),
                    connected_peers: state.endpoint.connected_peers(),
                    membership_active: active,
                    membership_passive: passive,
                    caches: CacheStats {
                        urns: state.caches.urns.stats(),
                    },
                })
                .ok();
            }
        },
    }
}

pub(super) async fn interrogation<S>(
    state: State<S>,
    event::downstream::Interrogation {
        peer: (peer, addr_hints),
        request,
        reply,
    }: event::downstream::Interrogation,
) where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
{
    let chan = reply.lock().take();
    if let Some(tx) = chan {
        let resp = match state.connection(peer, addr_hints).await {
            None => Err(error::Interrogation::NoConnection(peer)),
            Some(conn) => match io::send::request(&conn, request).await {
                Err(e) => Err(e.into()),
                Ok(resp) => resp.ok_or(error::Interrogation::NoResponse(peer)),
            },
        };
        tx.send(resp).ok();
    }
}

pub(super) async fn connect<S>(
    state: &State<S>,
    event::downstream::Connect {
        peer: (peer, addr_hints),
        reply,
    }: event::downstream::Connect,
) where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
{
    let chan = reply.lock().take();
    if let Some(tx) = chan {
        let conn = state.connection(peer, addr_hints).await;
        tx.send(conn).ok();
    }
}
