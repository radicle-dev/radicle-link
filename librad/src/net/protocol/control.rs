// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{iter, net::SocketAddr};

use futures::{
    channel::oneshot,
    stream::{self, BoxStream, StreamExt as _},
};

use super::{
    broadcast,
    error,
    event,
    gossip,
    graft,
    io,
    quic,
    tick,
    PeerInfo,
    ProtocolStorage,
    State,
};
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
        advertised_info: io::peer_advertisement(&state.endpoint),
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
    use event::downstream::{Info, MembershipInfo, Stats};

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
        let may_conn = io::connections::get_or_connect(&state, peer, addr_hints).await;
        let resp = match may_conn {
            None => Err(error::Interrogation::NoConnection(peer)),
            Some(conn) => match io::send::request(&conn, request).await {
                Err(e) => Err(e.into()),
                Ok(resp) => resp.ok_or(error::Interrogation::NoResponse(peer)),
            },
        };
        tx.send(resp).ok();
    }
}

pub(super) async fn graft<S>(
    state: &mut State<S>,
    event::downstream::Graft {
        peer: (peer, addr_hints),
        reply,
    }: event::downstream::Graft,
) where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
{
    // When global grafting is disabled, we still need to funnel through some
    // chans in order to get an owned stream back.
    async fn disabled<S>(
        state: State<S>,
        conn: quic::Connection,
    ) -> Result<BoxStream<'static, io::graft::Progress>, error::Graft>
    where
        S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
    {
        use graft::Grafting as _;

        let (tx, rx) = oneshot::channel();
        let cfg = io::graft::Config::from(state.config);
        state
            .spawner
            .clone()
            .spawn({
                let task = io::graft::Grafting::new(state, cfg).graft(None);
                async move { graft::schedule(&task, conn, tx).await }
            })
            .detach();

        Ok(rx
            .await
            .map_err(|_: oneshot::Canceled| error::Graft::Stopped)??
            .boxed())
    }

    let go = async {
        let conn = io::connections::get_or_connect(state, peer, addr_hints)
            .await
            .ok_or(error::Graft::NoConnection(peer))?;
        match state.graftq.as_mut() {
            Some(queue) => Ok(queue.push(conn)?.await??.boxed()),
            None => disabled(state.clone(), conn).await,
        }
    };

    let chan = reply.lock().take();
    if let Some(tx) = chan {
        let resp = go.await;
        tx.send(resp).ok();
    }
}
