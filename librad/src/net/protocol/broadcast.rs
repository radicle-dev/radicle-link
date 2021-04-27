// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::fmt::Debug;

use thiserror::Error;

use super::{event::upstream as event, tick, PeerInfo};
use crate::PeerId;

mod storage;
pub use storage::{LocalStorage, PutResult};

#[derive(Clone, Debug, PartialEq, minicbor::Encode, minicbor::Decode)]
pub enum Message<Addr, Payload> {
    #[n(0)]
    #[cbor(array)]
    Have {
        #[n(0)]
        origin: PeerInfo<Addr>,
        #[n(1)]
        val: Payload,
    },

    #[n(1)]
    #[cbor(array)]
    Want {
        #[n(0)]
        origin: PeerInfo<Addr>,
        #[n(1)]
        val: Payload,
    },
}

pub(super) trait Membership {
    fn members(&self, exclude: Option<PeerId>) -> Vec<PeerId>;
    fn is_member(&self, peer: &PeerId) -> bool;
}

pub(super) trait ErrorRateLimited {
    fn is_error_rate_limit_breached(&self) -> bool;
}

#[derive(Debug, Error)]
pub enum Error<A, P>
where
    A: Debug,
    P: Debug,
{
    #[error("unsolicited message from {remote_id}")]
    Unsolicited {
        remote_id: PeerId,
        message: Message<A, P>,
    },
}

#[tracing::instrument(skip(membership, storage, info), err)]
pub(super) async fn apply<M, S, F, A, P>(
    membership: &M,
    storage: &S,
    info: &F,
    remote_id: PeerId,
    message: Message<A, P>,
) -> Result<(Option<event::Gossip<A, P>>, Vec<tick::Tock<A, P>>), Error<A, P>>
where
    M: Membership,
    S: LocalStorage<A, Update = P> + ErrorRateLimited,
    F: Fn() -> PeerInfo<A>,
    A: Clone + Debug + Send + 'static,
    P: Clone + Debug,
{
    use tick::Tock::*;
    use Message::*;
    use PutResult::*;

    if !membership.is_member(&remote_id) {
        return Err(self::Error::Unsolicited { remote_id, message });
    }

    let broadcast = |msg: Message<A, P>, exclude: Option<PeerId>| {
        membership
            .members(exclude)
            .into_iter()
            .map(|to| SendConnected {
                to,
                message: msg.clone().into(),
            })
            .collect::<Vec<_>>()
    };

    match message {
        Have { origin, val } => {
            let res = (*storage).put(origin.clone(), val.clone()).await;
            tracing::error!(origin = ?origin, res = ?res, "BROADCAST APPLY");
            let event = event::Gossip::Put {
                provider: origin.clone(),
                payload: val.clone(),
                result: res.clone(),
            };

            let tocks = match res {
                Applied(ap) => broadcast(
                    Have {
                        origin: info(),
                        val: ap,
                    },
                    Some(remote_id),
                ),

                Error => {
                    let mut tocks = Vec::new();
                    // Forward anyways, error is local
                    tocks.extend(broadcast(
                        Have {
                            origin,
                            val: val.clone(),
                        },
                        Some(remote_id),
                    ));

                    if storage.is_error_rate_limit_breached() {
                        tracing::warn!("error rate limit breached");
                    } else {
                        // Request retransmission
                        tocks.extend(broadcast(
                            Want {
                                origin: info(),
                                val,
                            },
                            None,
                        ));
                    }

                    tocks
                },

                Uninteresting => broadcast(Have { origin, val }, Some(remote_id)),
                Stale => vec![],
            };

            Ok((Some(event), tocks))
        },

        Want { origin, val } => {
            let have = storage.ask(val.clone()).await;
            let tocks = if have {
                let reply = Have {
                    origin: info(),
                    val,
                };

                if origin.peer_id == remote_id {
                    vec![SendConnected {
                        to: remote_id,
                        message: reply.into(),
                    }]
                } else {
                    // FIXME: if we cannot reach origin, we may still want to
                    // broadcast the `Have`, in the hopes that it will travel
                    // back the path it came here
                    vec![AttemptSend {
                        to: origin,
                        message: reply.into(),
                    }]
                }
            } else {
                broadcast(Want { origin, val }, Some(remote_id))
            };

            Ok((None, tocks))
        },
    }
}
