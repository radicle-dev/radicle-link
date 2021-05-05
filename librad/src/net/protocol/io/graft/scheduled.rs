// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicUsize, Ordering::Relaxed},
        Arc,
    },
};

use async_stream::stream;
use futures::{FutureExt as _, StreamExt as _};

use super::{error, Config};
use crate::{
    git::{
        identities,
        replication::{self, ReplicateResult},
        storage::fetcher,
    },
    identities::SomeUrn,
    net::{
        connection::{RemoteAddr as _, RemotePeer as _},
        protocol::{gossip, graft, interrogation, io, membership, ProtocolStorage, State},
        quic,
    },
};

pub type Step = Result<ReplicateResult, error::Step>;
pub type Progress = graft::Progress<Step>;
pub type Reply = graft::Reply<Step, error::Prepare>;
pub type Message = (quic::Connection, Reply);

pub(in crate::net::protocol) type Scheduler<S> = graft::Scheduler<Task<S>, Message>;
pub(in crate::net::protocol) type Queue<R, S> = graft::Queue<Env<R>, Task<S>, Message>;

#[derive(Clone)]
pub struct Env<R> {
    membership: membership::Hpv<R, SocketAddr>,
}

impl<R> Env<R> {
    pub fn new(membership: membership::Hpv<R, SocketAddr>) -> Self {
        Self { membership }
    }
}

impl<R> graft::Env for Env<R>
where
    R: rand::Rng + Clone,
{
    fn is_joining(&self) -> bool {
        self.membership.view_stats().0 < self.membership.params().max_active
    }
}

pub(in crate::net::protocol) struct Grafting<S> {
    state: State<S>,
    config: Config,
}

impl<S> Grafting<S> {
    pub fn new(state: State<S>, config: Config) -> Self {
        Self { state, config }
    }
}

impl<S> graft::Grafting for Grafting<S>
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
{
    type Task = Task<S>;

    fn graft(&self, tx: Option<graft::Trackings>) -> Self::Task {
        Task {
            state: self.state.clone(),
            config: self.config.clone(),
            trackings: tx,
            num_tracked: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[derive(Clone)]
pub(in crate::net::protocol) struct Task<S> {
    state: State<S>,
    config: Config,
    trackings: Option<graft::Trackings>,
    num_tracked: Arc<AtomicUsize>,
}

impl<S> Task<S> {
    async fn candidates(&self, conn: &quic::Connection) -> Result<Vec<SomeUrn>, error::Prepare>
    where
        S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
    {
        use error::Prepare::*;
        use interrogation::{Request, Response};

        let remote_id = conn.remote_peer_id();
        let remote_urns = {
            let resp = io::send::request(&conn, Request::GetUrns)
                .await?
                .ok_or(NoResponse { from: remote_id })?;
            match resp {
                Response::Urns(urns) => Ok(urns),
                _ => Err(InvalidResponse { from: remote_id }),
            }
        }?;
        let candidates = match self.trackings.as_ref() {
            Some(graft::Trackings::Urns(only)) => only
                .iter()
                .filter_map(|urn| remote_urns.contains(urn).then_some(urn))
                .cloned()
                .collect(),
            Some(graft::Trackings::Max(tracked))
                if self.num_tracked.load(Relaxed) > tracked.get() =>
            {
                tracing::warn!("max local trackings exceeded");
                vec![]
            }
            _ => {
                let storage = self.state.storage.get().await?;
                let mut count = 0;
                let candidates = identities::any::list_urns(storage.as_ref())?
                    .filter_map(|urn| {
                        count += 1;
                        urn.ok().and_then(|urn| {
                            let urn = SomeUrn::from(urn);
                            remote_urns.contains(&urn).then_some(urn)
                        })
                    })
                    .collect();
                self.num_tracked.store(count, Relaxed);
                candidates
            },
        };

        Ok(candidates)
    }
}

impl<S> graft::Task for Task<S>
where
    S: ProtocolStorage<SocketAddr, Update = gossip::Payload> + Clone + 'static,
{
    type Context = quic::Connection;
    type Error = error::Prepare;
    type Step = Result<ReplicateResult, error::Step>;

    fn run(&self, cx: Self::Context) -> graft::TaskFuture<Self::Step, Self::Error> {
        async move {
            let candidates = self.candidates(&cx).await?;
            tracing::info!("found {} candidates", candidates.len());

            let timeout = self.config.fetch_slot_wait_timeout;
            let replcfg = self.config.replication;

            let remote_id = cx.remote_peer_id();
            let remote_addr = cx.remote_addr();

            let action = stream! {
                for urn in candidates {
                    yield graft::Progress::Started { urn: urn.clone() };

                    let SomeUrn::Git(gurn) = urn.clone();
                    let res = fetcher::retrying(
                        &self.state.spawner,
                        &self.state.storage,
                        fetcher::PeerToPeer::new(gurn, remote_id, Some(remote_addr)),
                        timeout,
                        move |storage, fetcher| {
                            replication::replicate(&storage, fetcher, replcfg, None)
                                .map_err(error::Step::from)
                        },
                    )
                    .await
                    .map_err(error::Step::from)
                    .flatten();

                    yield graft::Progress::Finished { urn, res };
                }
            };

            Ok(action.boxed())
        }
        .boxed()
    }
}
