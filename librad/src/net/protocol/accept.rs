// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::net::SocketAddr;

use futures::stream::{self, StreamExt as _};

use super::{broadcast, event, gossip, io, membership, tick, PeerInfo, RecvError, State};
use crate::PeerId;

#[tracing::instrument(skip(state, disco))]
pub(super) async fn disco<S, D>(state: State<S>, disco: D)
where
    S: broadcast::LocalStorage<Update = gossip::Payload> + 'static,
    D: futures::Stream<Item = (PeerId, Vec<SocketAddr>)>,
{
    disco
        .for_each(|(peer, addrs)| {
            let state = state.clone();
            async move { io::discovered(state, peer, addrs).await }
        })
        .await
}

#[tracing::instrument(skip(state, tasks))]
pub(super) async fn periodic<S, P>(state: State<S>, tasks: P)
where
    S: broadcast::LocalStorage<Update = gossip::Payload> + 'static,
    P: futures::Stream<Item = membership::Periodic<SocketAddr>>,
{
    tasks
        .flat_map(|p| match p {
            membership::Periodic::RandomPromotion { candidates } => {
                tracing::info!("initiating random promotion");
                stream::iter(
                    candidates
                        .into_iter()
                        .map(|info| tick::Tock::AttemptSend {
                            to: info,
                            message: state
                                .membership
                                .hello(io::peer_advertisement(&state.endpoint))
                                .into(),
                        })
                        .collect::<Vec<_>>(),
                )
            },

            membership::Periodic::Shuffle(membership::Shuffle {
                recipient,
                sample,
                ttl,
            }) => {
                tracing::info!("initiating shuffle");
                stream::iter(vec![tick::Tock::SendConnected {
                    to: recipient,
                    message: membership::Message::Shuffle {
                        origin: PeerInfo {
                            peer_id: state.local_id,
                            advertised_info: io::peer_advertisement(&state.endpoint),
                            seen_addrs: Default::default(),
                        },
                        peers: sample,
                        ttl,
                    }
                    .into(),
                }])
            },
        })
        .for_each(|tock| tick::tock(state.clone(), tock))
        .await
}

#[tracing::instrument(skip(state, rx))]
pub(super) async fn ground_control<S, E>(state: State<S>, mut rx: E)
where
    S: broadcast::LocalStorage<Update = gossip::Payload> + 'static,
    E: futures::Stream<Item = Result<event::Downstream, RecvError>> + Unpin,
{
    use event::{
        downstream::{Gossip, Info, Stats},
        Downstream,
    };

    while let Some(x) = rx.next().await {
        match x {
            Err(RecvError::Closed) => {
                tracing::error!("deep space 9 lost contact");
                break;
            },

            Err(RecvError::Lagged(i)) => {
                tracing::warn!("skipped {} messages from ground control", i)
            },

            Ok(evt) => {
                let origin = PeerInfo {
                    peer_id: state.local_id,
                    advertised_info: io::peer_advertisement(&state.endpoint),
                    seen_addrs: Default::default(),
                };
                match evt {
                    // TODO: answer `Want`s from a provider cache
                    Downstream::Gossip(gossip) => {
                        let rpc = match gossip {
                            Gossip::Announce(payload) => broadcast::Message::Have {
                                origin,
                                val: payload,
                            },
                            Gossip::Query(payload) => broadcast::Message::Want {
                                origin,
                                val: payload,
                            },
                        };
                        stream::iter(state.membership.broadcast_recipients(None).into_iter().map(
                            |to| tick::Tock::SendConnected {
                                to,
                                message: rpc.clone().into(),
                            },
                        ))
                        .for_each(|tock| tick::tock(state.clone(), tock))
                        .await
                    },

                    Downstream::Info(info) => match info {
                        Info::ConnectedPeers(tx) => {
                            if let Some(tx) = tx.lock().take() {
                                tx.send(state.endpoint.peers()).ok();
                            }
                        },

                        Info::Stats(tx) => {
                            if let Some(tx) = tx.lock().take() {
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
                    },
                };
            },
        }
    }
}
