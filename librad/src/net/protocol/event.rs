// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::HashMap, net::SocketAddr};

use super::{broadcast, cache, error, gossip, interrogation, membership};
use crate::PeerId;

#[derive(Clone)]
pub enum Downstream {
    Gossip(downstream::Gossip),
    Info(downstream::Info),
    Interrogation(downstream::Interrogation),
}

pub mod downstream {
    use super::*;

    use std::sync::Arc;

    use parking_lot::Mutex;
    use tokio::sync::oneshot;

    pub type Reply<T> = Arc<Mutex<Option<oneshot::Sender<T>>>>;

    #[derive(Clone, Debug)]
    pub enum Gossip {
        Announce(gossip::Payload),
        Query(gossip::Payload),
    }

    impl Gossip {
        pub fn payload(self) -> gossip::Payload {
            match self {
                Self::Announce(p) => p,
                Self::Query(p) => p,
            }
        }
    }

    #[derive(Clone)]
    pub enum Info {
        ConnectedPeers(Reply<Vec<PeerId>>),
        Membership(Reply<MembershipInfo>),
        Stats(Reply<Stats>),
    }

    #[derive(Clone, Debug, Default)]
    pub struct MembershipInfo {
        pub active: Vec<PeerId>,
        pub passive: Vec<PeerId>,
    }

    #[derive(Clone, Debug, Default)]
    pub struct Stats {
        pub connections_total: usize,
        pub connected_peers: HashMap<PeerId, Vec<SocketAddr>>,
        pub membership_active: usize,
        pub membership_passive: usize,
        pub caches: CacheStats,
    }

    #[derive(Clone, Copy, Debug, Default)]
    pub struct CacheStats {
        pub urns: cache::urns::Stats,
    }

    #[derive(Clone)]
    pub struct Interrogation {
        pub peer: (PeerId, Vec<SocketAddr>),
        pub request: interrogation::Request,
        pub reply:
            Reply<Result<interrogation::Response<'static, SocketAddr>, error::Interrogation>>,
    }
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum Upstream {
    Endpoint(upstream::Endpoint),
    Gossip(Box<upstream::Gossip<SocketAddr, gossip::Payload>>),
    Membership(membership::Transition<SocketAddr>),
    Caches(upstream::Caches),
}

pub mod upstream {
    use super::*;

    use std::time::Duration;

    use futures::{pin_mut, FutureExt as _, StreamExt as _};
    use thiserror::Error;

    use crate::net::protocol::{PeerInfo, RecvError};

    #[derive(Clone, Debug)]
    pub enum Endpoint {
        Up { listen_addrs: Vec<SocketAddr> },
        Down,
    }

    impl From<Endpoint> for Upstream {
        fn from(e: Endpoint) -> Self {
            Self::Endpoint(e)
        }
    }

    #[derive(Clone, Debug)]
    pub enum Gossip<Addr, Payload> {
        /// Triggered after applying a `Have` to [`broadcast::LocalStorage`].
        Put {
            /// The peer who announced the `Have`
            provider: PeerInfo<Addr>,
            /// The payload we received (can only be a `Have`)
            payload: Payload,
            /// The result of applying to local storage
            result: broadcast::PutResult<Payload>,
        },
    }

    impl From<Gossip<SocketAddr, gossip::Payload>> for Upstream {
        fn from(g: Gossip<SocketAddr, gossip::Payload>) -> Self {
            Self::Gossip(Box::new(g))
        }
    }

    impl From<membership::Transition<SocketAddr>> for Upstream {
        fn from(t: membership::Transition<SocketAddr>) -> Self {
            Self::Membership(t)
        }
    }

    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum Caches {
        Urns(cache::urns::Event),
    }

    impl From<Caches> for Upstream {
        fn from(c: Caches) -> Self {
            Self::Caches(c)
        }
    }

    impl From<cache::urns::Event> for Upstream {
        fn from(e: cache::urns::Event) -> Self {
            Self::from(Caches::Urns(e))
        }
    }

    #[derive(Debug, Error)]
    pub enum ExpectError {
        #[error("timeout waiting for matching event")]
        Timeout,
        #[error("sender lost")]
        Lost,
    }

    pub async fn expect<S, P>(
        events: S,
        matching: P,
        timeout: Duration,
    ) -> Result<Upstream, ExpectError>
    where
        S: futures::Stream<Item = Result<Upstream, RecvError>> + Unpin,
        P: Fn(&Upstream) -> bool,
    {
        let timeout = link_async::sleep(timeout).fuse();
        pin_mut!(timeout);
        let mut events = events.fuse();
        loop {
            futures::select! {
                _ = timeout => return Err(ExpectError::Timeout),
                i = events.next() => match i {
                    Some(Ok(event)) if matching(&event) => return Ok(event),
                    Some(Err(RecvError::Closed)) | None => return Err(ExpectError::Lost),
                    _ => {
                        continue;
                    }
                }
            }
        }
    }

    pub mod predicate {
        use super::*;

        pub fn gossip_from(peer: PeerId) -> impl Fn(&Upstream) -> bool {
            move |event| match event {
                Upstream::Gossip(box Gossip::Put { provider, .. }) => provider.peer_id == peer,
                _ => false,
            }
        }

        /// Wait for cache `Rebuilt` events where the new length matches the
        /// predicate.
        pub fn urn_cache_len<P>(cmp: P) -> impl Fn(&Upstream) -> bool
        where
            P: Fn(usize) -> bool,
        {
            move |event| match event {
                Upstream::Caches(Caches::Urns(cache::urns::Event::Rebuilt { len_new, .. })) => {
                    cmp(*len_new)
                },
                _ => false,
            }
        }
    }
}
