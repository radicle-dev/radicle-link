// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::HashMap, net::SocketAddr};

use futures::stream::BoxStream;

use super::{broadcast, error, gossip, interrogation, io, membership};
use crate::PeerId;

#[derive(Clone)]
pub enum Downstream {
    Gossip(downstream::Gossip),
    Info(downstream::Info),
    Interrogation(downstream::Interrogation),
    Graft(downstream::Graft),
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
    }

    #[derive(Clone)]
    pub struct Interrogation {
        pub peer: (PeerId, Vec<SocketAddr>),
        pub request: interrogation::Request,
        pub reply:
            Reply<Result<interrogation::Response<'static, SocketAddr>, error::Interrogation>>,
    }

    #[derive(Clone)]
    pub struct Graft {
        pub peer: (PeerId, Vec<SocketAddr>),
        pub reply: Reply<Result<BoxStream<'static, io::graft::Progress>, error::Graft>>,
    }
}

#[derive(Clone, Debug)]
pub enum Upstream {
    Endpoint(upstream::Endpoint),
    Gossip(Box<upstream::Gossip<SocketAddr, gossip::Payload>>),
    Membership(membership::Transition<SocketAddr>),
}

pub mod upstream {
    use super::*;

    use std::time::Duration;

    use futures::{FutureExt as _, StreamExt as _};
    use futures_timer::Delay;
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
        let mut timeout = Delay::new(timeout).fuse();
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
    }
}
