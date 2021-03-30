// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::net::SocketAddr;

use futures::stream::{self, StreamExt as _};

use super::{
    control,
    event,
    gossip,
    io,
    membership,
    tick,
    PeerInfo,
    ProtocolStorage,
    RecvError,
    State,
};
use crate::PeerId;

#[tracing::instrument(skip(state, disco))]
pub(super) async fn disco<S, D>(state: State<S>, disco: D)
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + 'static,
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
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + 'static,
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
        .await;

    // Tickle connections in the partial view.
    //
    // This is mostly to keep passive connections from being collected. Note
    // that we're not checking actual liveness, nor interfering with the
    // membership protocol.
    for peer in state.membership.known() {
        if let Some(conn) = state.endpoint.get_connection(peer) {
            conn.tickle()
        }
    }
}

#[tracing::instrument(skip(state, rx))]
pub(super) async fn ground_control<S, E>(state: State<S>, mut rx: E)
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + 'static,
    E: futures::Stream<Item = Result<event::Downstream, RecvError>> + Unpin,
{
    use event::Downstream;

    while let Some(x) = rx.next().await {
        match x {
            Err(RecvError::Closed) => {
                tracing::error!("deep space 9 lost contact");
                break;
            },

            Err(RecvError::Lagged(i)) => {
                tracing::warn!("skipped {} messages from ground control", i)
            },

            Ok(evt) => match evt {
                Downstream::Gossip(gossip) => control::gossip(&state, gossip).await,
                Downstream::Info(info) => control::info(&state, info),
                Downstream::Interrogation(inter) => {
                    control::interrogation(state.clone(), inter).await
                },
            },
        }
    }
}
