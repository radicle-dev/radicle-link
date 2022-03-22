// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{iter, net::SocketAddr};

use futures::stream::{self, StreamExt as _};

use super::{
    broadcast,
    error,
    event,
    gossip,
    interrogation,
    io,
    request_pull,
    tick,
    PeerInfo,
    ProtocolStorage,
    RequestPullGuard,
    State,
};
use crate::PeerId;

pub(super) async fn gossip<S, G>(
    state: &State<S, G>,
    evt: event::downstream::Gossip,
    exclude: Option<PeerId>,
) where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + 'static,
    G: RequestPullGuard,
{
    use event::downstream::Gossip;

    let origin = PeerInfo {
        peer_id: state.local_id,
        advertised_info: io::peer_advertisement(&state.endpoint)(),
        seen_addrs: iter::empty().into(),
    };
    // TODO: answer `Want`s from a provider cache
    let rpc = match evt {
        Gossip::Announce(payload) => broadcast::Message::have(origin, payload),
        Gossip::Query(payload) => broadcast::Message::want(origin, payload),
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

pub(super) fn info<S, G>(state: &State<S, G>, evt: event::downstream::Info)
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

pub(super) async fn interrogation<S, G>(
    state: State<S, G>,
    event::downstream::Interrogation {
        peer: (peer, addr_hints),
        request,
        reply,
    }: event::downstream::Interrogation,
) where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
    G: RequestPullGuard,
{
    let chan = reply.lock().take();
    if let Some(tx) = chan {
        let resp = match state.connection(peer, addr_hints).await {
            None => Err(error::Interrogation::NoConnection(peer)),
            Some(conn) => {
                match io::send::single_response(&conn, request, interrogation::FRAMED_BUFSIZ).await
                {
                    Err(e) => Err(e.into()),
                    Ok(resp) => resp.ok_or(error::Interrogation::NoResponse(peer)),
                }
            },
        };
        tx.send(resp).ok();
    }
}

pub(super) async fn request_pull<S, G>(
    state: State<S, G>,
    event::downstream::RequestPull {
        peer: (peer, addr_hints),
        request,
        reply,
    }: event::downstream::RequestPull,
) where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
    G: RequestPullGuard,
{
    let chan = reply.lock().take();
    if let Some(tx) = chan {
        match state.connection(peer, addr_hints).await {
            None => {
                tx.send(Err(error::RequestPull::NoConnection(peer)))
                    .await
                    .ok();
            },
            Some(conn) => {
                match io::send::multi_response(&conn, request, request_pull::FRAMED_BUFSIZ).await {
                    Err(e) => {
                        tx.send(Err(e.into())).await.ok();
                    },
                    Ok(mut resp) => {
                        while let Some(r) = resp.next().await {
                            tx.send(r.map_err(|e| e.into())).await.ok();
                        }
                    },
                }
            },
        };
    }
}

pub(super) async fn connect<S, G>(
    state: &State<S, G>,
    event::downstream::Connect {
        peer: (peer, addr_hints),
        reply,
    }: event::downstream::Connect,
) where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
    G: RequestPullGuard,
{
    let chan = reply.lock().take();
    if let Some(tx) = chan {
        let conn = state.connection(peer, addr_hints).await;
        tx.send(conn).ok();
    }
}
