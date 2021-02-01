// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{net::SocketAddr, time::Instant};

use futures::stream::{self, StreamExt as _};
use futures_timer::Delay;

use super::{broadcast, error, event, gossip, io, tick, PeerInfo, ProtocolStorage, State};
use crate::PeerId;

pub(super) async fn gossip<S>(state: &State<S>, evt: event::downstream::Gossip)
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + 'static,
{
    use event::downstream::Gossip;

    let origin = PeerInfo {
        peer_id: state.local_id,
        advertised_info: io::peer_advertisement(&state.endpoint),
        seen_addrs: Default::default(),
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
            .broadcast_recipients(None)
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
    use event::downstream::{Info, Stats};

    match evt {
        Info::ConnectedPeers(reply) => {
            let chan = reply.lock().take();
            if let Some(tx) = chan {
                tx.send(state.endpoint.peers()).ok();
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

pub(super) async fn state<S>(state: State<S>, evt: event::downstream::State)
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + 'static,
{
    use event::downstream::State;

    match evt {
        State::GraftReset { when, reply } => {
            let chan = reply.lock().take();
            if let Some(tx) = chan {
                tokio::spawn(async move {
                    let res = graft::reset(state, when).await;
                    tx.send(res).ok();
                });
            }
        },

        State::GraftInitiate {
            remote_id,
            addr_hints,
            reply,
        } => {
            let chan = reply.lock().take();
            if let Some(tx) = chan {
                let res = graft::initiate(&state, remote_id, addr_hints).await;
                tx.send(res).ok();
            }
        },
    }
}

mod graft {
    use super::*;

    pub(super) async fn reset<S>(
        state: State<S>,
        when: event::downstream::GraftResetPolicy,
    ) -> Result<(), error::GraftReset>
    where
        S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + 'static,
    {
        use event::downstream::GraftResetPolicy::*;

        let delay = match when {
            Now => None,
            Expired => state
                .graft
                .read()
                .deadline()
                .checked_duration_since(Instant::now())
                .map(Delay::new),
        };
        if let Some(wait) = delay {
            wait.await
        }
        let storage = state.storage.get().await?;
        Ok(state.graft.write().reset(storage.as_ref())?)
    }

    pub(super) async fn initiate<S>(
        state: &State<S>,
        remote_id: PeerId,
        addr_hints: Vec<SocketAddr>,
    ) -> Result<(), error::GraftInitiate>
    where
        S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + 'static,
    {
        let conn = match state.endpoint.get_connection(remote_id) {
            None => io::connect(&state.endpoint, remote_id, addr_hints)
                .await
                .map(|(conn, ingress)| {
                    tokio::spawn(io::ingress_streams(state.clone(), ingress));
                    conn
                }),

            Some(conn) => Some(conn),
        }
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotConnected, "could not connect")
        })?;
        io::initiate_graft(state, conn).await
    }
}
