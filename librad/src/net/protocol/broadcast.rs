// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::hash_map::RandomState,
    fmt::Debug,
    hash::{Hash, Hasher},
    sync::Arc,
};

use bloom_filters::{DefaultBuildHashKernels, StableBloomFilter};
use parking_lot::RwLock;
use thiserror::Error;
use tracing::{debug, warn};

use super::{event::upstream as event, tick, PeerInfo};
use crate::{PeerId, Signature};

mod metrics;
pub use metrics::Metrics;

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
        #[n(2)]
        ext: Option<Ext>,
    },

    #[n(1)]
    #[cbor(array)]
    Want {
        #[n(0)]
        origin: PeerInfo<Addr>,
        #[n(1)]
        val: Payload,
        #[n(2)]
        ext: Option<Ext>,
    },
}

impl<A, P> Message<A, P> {
    pub fn have(origin: PeerInfo<A>, val: P) -> Self {
        Self::Have {
            origin,
            val,
            ext: Some(Ext::new()),
        }
    }

    pub fn want(origin: PeerInfo<A>, val: P) -> Self {
        Self::Want {
            origin,
            val,
            ext: Some(Ext::new()),
        }
    }

    pub fn origin(&self) -> &PeerInfo<A> {
        match self {
            Self::Have { origin, .. } | Self::Want { origin, .. } => origin,
        }
    }

    pub fn payload(&self) -> &P {
        match self {
            Self::Have { val, .. } | Self::Want { val, .. } => val,
        }
    }

    pub fn hop_count(&self) -> Option<usize> {
        self.ext().as_ref().map(|x| x.hop_count())
    }

    pub fn ext(&self) -> Option<&Ext> {
        match self {
            Self::Have { ext, .. } | Self::Want { ext, .. } => ext.as_ref(),
        }
    }
}

impl<A, P: Hash> Hash for Message<A, P> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        self.origin().peer_id.hash(state);
        match self.ext() {
            None => self.payload().hash(state),
            Some(ext) => ext.seqno.hash(state),
        }
    }
}

// We should've defined `Message` as a flat struct with a discriminator field.
// Since we didn't, introducing v2 extensions through an indirection is slightly
// more convenient, as we don't have to deal with all fields being optional.
#[derive(Clone, Debug, PartialEq, minicbor::Encode, minicbor::Decode)]
#[cbor(array)]
pub struct Ext {
    /// Sequence number of the [`Message`], unique per [`Message::origin`].
    ///
    /// This can be a monotonic counter when message signing is used, otherwise
    /// must be a random value.
    #[n(0)]
    seqno: u64,
    /// Hop count of the [`Message`], incremented by each recipient.
    #[n(1)]
    hop: usize,
    /// Message signature. Not yet supported.
    #[n(2)]
    sig: Option<Signature>,
}

impl Ext {
    pub fn new() -> Self {
        Self {
            seqno: rand::random(),
            hop: 0,
            sig: None,
        }
    }

    pub fn next_hop(self) -> Self {
        Self {
            hop: self.hop.saturating_add(1),
            ..self
        }
    }

    pub fn hop_count(&self) -> usize {
        self.hop
    }
}

impl Default for Ext {
    fn default() -> Self {
        Self::new()
    }
}

pub(super) trait Membership {
    fn members(&self, exclude: Option<PeerId>) -> Vec<PeerId>;
    fn is_member(&self, peer: &PeerId) -> bool;
}

pub enum Limit<'a> {
    Errors,
    Wants { recipient: &'a PeerId },
}

pub(super) trait RateLimited {
    fn is_rate_limit_breached(&self, lim: Limit) -> bool;
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

type SeenFilter = StableBloomFilter<DefaultBuildHashKernels<RandomState>>;

#[derive(Clone)]
pub struct State<S, T> {
    storage: S,
    seen: Arc<RwLock<SeenFilter>>,
    stats: T,
    // TODO: move rate limiters into here
}

impl<S, T> State<S, T> {
    pub fn new(storage: S, stats: T) -> Self {
        Self {
            storage,
            // Parameters are from the SBF paper, with Max=3 due to the
            // expectation that we will get duplicates in quick succession, and
            // then late comers. FPR is a bit more aggressive.
            //
            // Requires about 3.75MiB memory.
            seen: Arc::new(RwLock::new(SeenFilter::new(
                10_000_000,
                3,
                0.001,
                DefaultBuildHashKernels::new(rand::random(), RandomState::new()),
            ))),
            stats,
        }
    }
}

impl<S, T> State<S, T>
where
    T: Metrics,
{
    fn seen<A, P: Hash>(&self, m: &Message<A, P>) -> bool {
        use bloom_filters::BloomFilter as _;

        let filter = self.seen.read();
        if filter.contains(m) {
            self.record_seen();
            true
        } else {
            drop(filter);
            let mut filter = self.seen.write();
            filter.insert(m);

            false
        }
    }

    pub(super) async fn apply<M, F, A, P>(
        &self,
        membership: &M,
        info: F,
        remote_id: PeerId,
        message: Message<A, P>,
    ) -> Result<(Option<event::Gossip<A, P>>, Vec<tick::Tock<A, P>>), Error<A, P>>
    where
        S: LocalStorage<A, Update = P> + RateLimited,
        M: Membership,
        F: Fn() -> PeerInfo<A>,
        A: Clone + Debug + Send + 'static,
        P: Clone + Debug + Hash,
    {
        apply(self, membership, info, remote_id, message).await
    }
}

impl<S, T> Metrics for State<S, T>
where
    T: Metrics,
{
    type Snapshot = T::Snapshot;

    fn record_message(&self, hop_count: Option<usize>) {
        self.stats.record_message(hop_count)
    }

    fn record_seen(&self) {
        self.stats.record_seen()
    }

    fn snapshot(&self) -> Self::Snapshot {
        self.stats.snapshot()
    }
}

#[tracing::instrument(skip(state, membership, info))]
pub(super) async fn apply<S, T, M, F, A, P>(
    state: &State<S, T>,
    membership: &M,
    info: F,
    remote_id: PeerId,
    message: Message<A, P>,
) -> Result<(Option<event::Gossip<A, P>>, Vec<tick::Tock<A, P>>), Error<A, P>>
where
    S: LocalStorage<A, Update = P> + RateLimited,
    T: Metrics,
    M: Membership,
    F: Fn() -> PeerInfo<A>,
    A: Clone + Debug + Send + 'static,
    P: Clone + Debug + Hash,
{
    use tick::Tock::*;
    use Message::*;
    use PutResult::*;

    state.record_message(message.hop_count());
    if state.seen(&message) {
        debug!(?message, "seen previously");
        return Ok((None, vec![]));
    }

    if !membership.is_member(&remote_id) {
        return Err(self::Error::Unsolicited { remote_id, message });
    }

    let State { storage, .. } = state;

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
        Have { origin, val, ext } => {
            let res = storage.put(origin.clone(), val.clone()).await;
            let event = event::Gossip::Put {
                provider: origin.clone(),
                payload: val.clone(),
                result: res.clone(),
            };

            let tocks = match res {
                Applied(ap) => broadcast(Message::have(info(), ap), Some(remote_id)),

                Error => {
                    let mut tocks = Vec::new();
                    // Forward anyways, error is local
                    tocks.extend(broadcast(
                        Have {
                            origin,
                            val: val.clone(),
                            ext: Some(ext.unwrap_or_default().next_hop()),
                        },
                        Some(remote_id),
                    ));

                    if storage.is_rate_limit_breached(Limit::Errors) {
                        warn!("error rate limit breached");
                    } else {
                        // Request retransmission
                        tocks.extend(broadcast(Message::want(info(), val), None));
                    }

                    tocks
                },

                Uninteresting => broadcast(
                    Have {
                        origin,
                        val,
                        ext: Some(ext.unwrap_or_default().next_hop()),
                    },
                    Some(remote_id),
                ),

                Stale => vec![],
            };

            Ok((Some(event), tocks))
        },

        Want { origin, val, ext } => {
            if storage.is_rate_limit_breached(Limit::Wants {
                recipient: &origin.peer_id,
            }) {
                warn!(
                    "want rate limit breached: enhance your calm, {}!",
                    origin.peer_id
                );
                return Ok((None, vec![]));
            }

            let have = storage.ask(val.clone()).await;
            let tocks = if have {
                let reply = Message::have(info(), val);
                if origin.peer_id == remote_id {
                    vec![SendConnected {
                        to: remote_id,
                        message: reply.into(),
                    }]
                } else {
                    broadcast(reply, Some(remote_id))
                }
            } else {
                broadcast(
                    Want {
                        origin,
                        val,
                        ext: Some(ext.unwrap_or_default().next_hop()),
                    },
                    Some(remote_id),
                )
            };

            Ok((None, tocks))
        },
    }
}
