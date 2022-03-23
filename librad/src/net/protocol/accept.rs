// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{iter, net::SocketAddr};

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
    RequestPullGuard,
    State,
};
use crate::PeerId;

#[tracing::instrument(skip(state, disco))]
pub(super) async fn disco<S, G, D>(state: State<S, G>, disco: D)
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + 'static,
    G: RequestPullGuard,
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
pub(super) async fn periodic<S, G, P>(state: State<S, G>, tasks: P)
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + 'static,
    G: RequestPullGuard,
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
                                .hello(io::peer_advertisement(&state.endpoint)())
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
                            advertised_info: io::peer_advertisement(&state.endpoint)(),
                            seen_addrs: iter::empty().into(),
                        },
                        peers: sample,
                        ttl,
                    }
                    .into(),
                }])
            },

            membership::Periodic::Tickle => {
                // Tickle connections in the partial view.
                //
                // This is mostly to keep passive connections from being collected. Note
                // that we're not checking actual liveness, nor interfering with the
                // membership protocol.
                tracing::debug!("initiating tickle");
                for peer in state.membership.known() {
                    if let Some(conn) = state.endpoint.get_connection(peer) {
                        conn.tickle();
                    }
                }

                // There are no tocks to evaluate for this case, there is opportunity to improve
                // this part as sugested in [comment]:
                //
                //      This is fine. If it looks funny to you, then because accept also
                // evaluates      the tocks. More cleansily, this could be two
                // functions, one which transforms      the periodic events into
                // tocks, and another one feeds them to the tock-evaluator.
                //
                // [comment]: https://github.com/radicle-dev/radicle-link/pull/615#discussion_r614402283
                stream::iter(vec![])
            },
        })
        .for_each(|tock| tick::tock(state.clone(), tock))
        .await;
}

#[tracing::instrument(skip(state, rx))]
pub(super) async fn ground_control<S, G, E>(state: State<S, G>, rx: E)
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + 'static,
    G: RequestPullGuard,
    E: futures::Stream<Item = Result<event::Downstream, RecvError>>,
{
    use event::Downstream;

    futures::pin_mut!(rx);
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
                Downstream::Gossip(x) => control::gossip(&state, x, None).await,
                Downstream::Info(x) => control::info(&state, x),
                Downstream::Interrogation(x) => control::interrogation(state.clone(), x).await,
                Downstream::RequestPull(x) => control::request_pull(state.clone(), x).await,
                Downstream::Connect(x) => control::connect(&state, x).await,
            },
        }
    }
}
