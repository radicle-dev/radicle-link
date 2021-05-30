// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::BTreeSet,
    fmt::Debug,
    future::Future,
    marker::PhantomData,
    num::NonZeroUsize,
    ops::Try,
    sync::Arc,
};

use data::NonEmpty;
use futures::{
    channel::{mpsc, oneshot},
    future::{BoxFuture, TryFutureExt as _},
    stream::{BoxStream, Stream, StreamExt as _},
};

use crate::{executor, identities::SomeUrn, net::connection::RemotePeer, PeerId};

pub mod error;

#[derive(Clone, Debug)]
pub struct Config {
    /// Maximum number of queued grafts.
    pub max_backlog: usize,
    /// [`Policies`] configuration.
    pub policies: Policies,
    /// Additional [`Constraints`].
    pub constraints: Constraints,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_backlog: 32,
            policies: Default::default(),
            constraints: Default::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Policies {
    /// The [`Policy`] to apply when a new peer is discovered.
    ///
    /// Note that this only applies to out-of-band discovery, not the membership
    /// protocol.
    pub discovered: Policy,
    /// The [`Policy`] to apply when a new incoming connection is established.
    pub incoming: Policy,
    /// The [`Policy`] to apply when a new outgoing connection is established.
    ///
    /// Note that this only applies to protocol-initiated connections.
    pub outgoing: Policy,
}

impl Policies {
    pub(crate) fn for_source(&self, src: &Source) -> Policy {
        match src {
            Source::Discovery => self.discovered,
            Source::Incoming => self.incoming,
            Source::Outgoing => self.outgoing,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Policy {
    /// Always initiate a graft procedure.
    Always,
    /// Only initiate a graft while the partial view is below capacity. This is
    /// the default.
    Joining,
}

impl Default for Policy {
    fn default() -> Self {
        Self::Joining
    }
}

#[derive(Clone, Debug, Default)]
pub struct Constraints {
    /// Only consider the given peers for grafting.
    ///
    /// Useful when using discovery with a set of trusted peers, and not very
    /// useful otherwise. Default: unrestricted
    pub peers: Option<NonEmpty<BTreeSet<PeerId>>>,
    /// Restrict by local tracking state. Default: unrestricted
    pub trackings: Option<Trackings>,
}

#[derive(Clone, Debug)]
pub enum Trackings {
    /// Stop grafting when a maximum number of tracked URNs has been reached.
    ///
    /// Useful default for long-running or pre-configured peers: since the
    /// interrogation response has to be probed element-wise against the
    /// locally tracked URNs, it is advisable to restrict graft initiation
    /// while having low cardinality as compared to the maximum permissible URNs
    /// in an interrogation response.
    Max(NonZeroUsize),
    /// Only probe for a fixed set of URNs.
    ///
    /// Particularly useful for seed nodes with liberal tracking policies: their
    /// cardinality is most likely prohibitively high, but they may wish to
    /// provide eager replication services for a limited set of URNs without
    /// incurring IOPS and limiting computational cost.
    Urns(NonEmpty<BTreeSet<SomeUrn>>),
}

pub struct Trigger<C> {
    pub context: C,
    pub source: Source,
}

pub enum Source {
    Discovery,
    Incoming,
    Outgoing,
}

pub trait Env {
    fn is_joining(&self) -> bool;
}

pub trait Grafting {
    type Task: Task;

    fn graft(&self, tx: Option<Trackings>) -> Self::Task;
}

pub type TaskFuture<'a, S, E> = BoxFuture<'a, Result<BoxStream<'a, Progress<S>>, E>>;

pub trait Task {
    type Context;
    type Error: std::error::Error + Send;
    type Step: Try + Send;

    fn run(&self, cx: Self::Context) -> TaskFuture<Self::Step, Self::Error>;
}

pub enum Progress<R> {
    Started { urn: SomeUrn },
    Finished { urn: SomeUrn, res: R },
}

pub fn guard_policy<E>(config: &Config, env: &E, source: &Source) -> Result<(), error::Policy>
where
    E: Env,
{
    match config.policies.for_source(source) {
        Policy::Joining => env.is_joining(),
        Policy::Always => true,
    }
    .then(|| ())
    .ok_or(error::Policy)
}

pub fn guard_constraints<C>(config: &Config, cx: &C) -> Result<(), error::Policy>
where
    C: RemotePeer,
{
    let remote_id = cx.remote_peer_id();
    config
        .constraints
        .peers
        .as_ref()
        .map(|set| set.contains(&remote_id))
        .unwrap_or(true)
        .then(|| ())
        .ok_or(error::Policy)
}

pub struct Queue<E, T, M> {
    spawner: Arc<executor::Spawner>,
    config: Config,
    env: E,
    tx: mpsc::Sender<M>,
    _task: PhantomData<T>,
}

impl<E, T, M> Clone for Queue<E, T, M>
where
    E: Clone,
{
    fn clone(&self) -> Self {
        Self {
            spawner: Arc::clone(&self.spawner),
            config: self.config.clone(),
            env: self.env.clone(),
            tx: self.tx.clone(),
            _task: PhantomData,
        }
    }
}

pub(crate) type Reply<S, E> = oneshot::Sender<Result<mpsc::UnboundedReceiver<Progress<S>>, E>>;

type Message<T> = (
    <T as Task>::Context,
    Reply<<T as Task>::Step, <T as Task>::Error>,
);

impl<E, T> Queue<E, T, Message<T>>
where
    E: Env,
    T: Task,
    T::Context: Debug,
{
    pub fn new(
        spawner: Arc<executor::Spawner>,
        mut config: Config,
        env: E,
    ) -> (Self, BuildScheduler<T, Message<T>>) {
        let (tx, rx) = mpsc::channel(config.max_backlog);
        let recv = BuildScheduler::new(config.constraints.trackings.take(), rx);
        let this = Self {
            spawner,
            config,
            env,
            tx,
            _task: PhantomData,
        };

        (this, recv)
    }

    /// Enqueue a grafting task.
    ///
    /// This does **not** evaluate [`Config::policies`] nor
    /// [`Config::constraints`], but may return an error if too many tasks
    /// are already in-flight, or the [`Scheduler`] died unexpectedly.
    ///
    /// The returned future will resolve once the task gets scheduled. If the
    /// scheduler was stopped by that time, it resolves to an error.
    /// Otherwise, it resolves to another [`Result`]: an error if task setup
    /// failed, or a stream of [`Progress`] events.
    pub fn push(
        &mut self,
        cx: T::Context,
    ) -> Result<
        impl Future<
            Output = Result<
                Result<impl Stream<Item = Progress<T::Step>>, T::Error>,
                error::Scheduler,
            >,
        >,
        error::Queue<T::Context>,
    > {
        let (rtx, rrx) = oneshot::channel();
        self.tx.try_send((cx, rtx))?;

        Ok(rrx.err_into())
    }

    /// [`Trigger`] a grafting task.
    ///
    /// Subject to [`Policies`], [`Constraints`], and queue depth.
    #[tracing::instrument(skip(self, trigger))]
    pub(crate) fn trigger(
        &mut self,
        trigger: Trigger<T::Context>,
    ) -> Result<(), error::Trigger<T::Context>>
    where
        E: 'static,
        T: 'static,
        T::Context: RemotePeer,
        T::Error: Debug,
        T::Step: Debug,
    {
        guard_policy(&self.config, &self.env, &trigger.source)?;
        guard_constraints(&self.config, &trigger.context)?;

        let reply = self.push(trigger.context)?;
        self.spawner.spawn(observe(reply)).detach();

        Ok(())
    }
}

async fn observe<F, P, S, E>(task: F)
where
    F: Future<Output = Result<Result<P, E>, error::Scheduler>>,
    P: Stream<Item = Progress<S>> + Unpin,
    S: Debug,
    E: Debug,
{
    match task.await {
        Err(error::Scheduler::Cancelled) => tracing::info!("task cancelled"),
        Ok(Err(e)) => tracing::warn!(err = ?e, "task setup failed"),
        Ok(Ok(mut progress)) => {
            while let Some(p) = progress.next().await {
                match p {
                    Progress::Started { urn } => tracing::info!("start {}", urn),
                    Progress::Finished { urn, res } => {
                        tracing::info!("finish {}", urn);
                        tracing::debug!("result {:?}", res)
                    },
                }
            }
        },
    }
}

#[must_use]
pub struct BuildScheduler<T, M> {
    constraints: Option<Trackings>,
    rx: mpsc::Receiver<M>,
    _task: PhantomData<T>,
}

impl<T, M> BuildScheduler<T, M> {
    pub(crate) fn new(constraints: Option<Trackings>, rx: mpsc::Receiver<M>) -> Self {
        Self {
            constraints,
            rx,
            _task: PhantomData,
        }
    }
}

impl<T, M> BuildScheduler<T, M> {
    pub fn build<G>(self, grafting: G) -> Scheduler<G::Task, M>
    where
        G: Grafting<Task = T>,
    {
        let task = grafting.graft(self.constraints);
        Scheduler { task, rx: self.rx }
    }
}

pub struct Scheduler<T, M> {
    task: T,
    rx: mpsc::Receiver<M>,
}

impl<T> Scheduler<T, Message<T>>
where
    T: Task,
{
    pub async fn run(self) {
        let Self { task, mut rx } = self;
        while let Some((cx, re)) = rx.next().await {
            schedule(&task, cx, re).await
        }
    }
}

pub(crate) async fn schedule<T, C, S, E>(task: &T, cx: C, re: Reply<S, E>)
where
    T: Task<Context = C, Step = S, Error = E>,
{
    match task.run(cx).await {
        Err(e) => {
            re.send(Err(e)).ok();
        },
        Ok(mut progress) => {
            let (ptx, prx) = mpsc::unbounded();
            re.send(Ok(prx)).ok();
            while let Some(p) = progress.next().await {
                ptx.unbounded_send(p).ok();
            }
        },
    }
}
