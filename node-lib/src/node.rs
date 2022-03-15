// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{panic, sync::Arc, time::Duration};

use clap::Parser as _;
use futures::{future::FutureExt as _, stream::FuturesUnordered, StreamExt};
use tokio::sync::mpsc;
use tracing::info;

use librad::{
    crypto::BoxedSigner,
    net::{discovery, peer::Peer},
};

use crate::{
    api,
    args::Args,
    cfg::{self, Cfg, RunMode},
    logging,
    metrics::graphite,
    protocol,
    request_pull,
    signals,
    tracking,
};

/// The amount of time to wait for connections before making any announcements
static ANNOUNCE_WAIT_TIME: Duration = Duration::from_secs(5);

pub async fn run() -> anyhow::Result<()> {
    logging::init();

    let spawner = Arc::new(link_async::Spawner::from_current().unwrap());

    let args = Args::parse();
    let cfg: Cfg<discovery::Static, BoxedSigner, request_pull::State> = cfg(&args).await?;

    let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
    let mut signals_task = spawner.spawn(signals::routine(shutdown_tx)).fuse();

    let mut coalesced = FuturesUnordered::new();
    let peer = Peer::new(cfg.peer)?;
    let peer_task = spawner
        .spawn(protocol::routine(peer.clone(), cfg.disco, shutdown_rx))
        .fuse();
    coalesced.push(peer_task);

    if let Some(cfg::Metrics::Graphite(addr)) = cfg.metrics {
        let graphite_task = spawner.spawn(graphite::routine(peer.clone(), addr)).fuse();
        coalesced.push(graphite_task);
    }

    if let Some(tracker) = cfg.tracker {
        let tracking_task = spawner
            .spawn(tracking::routine(peer.clone(), tracker))
            .fuse();
        coalesced.push(tracking_task);
    }

    let timeout = match cfg.run_mode {
        RunMode::Mortal(t) => Some(t),
        RunMode::Immortal => None,
    };
    let sockets = api::Sockets::load(spawner.clone(), &cfg.profile, peer.peer_id()).await?;
    let api_routine = api::routine(
        spawner.clone(),
        peer.clone(),
        &sockets,
        timeout,
        ANNOUNCE_WAIT_TIME,
    )
    .fuse();

    futures::pin_mut!(api_routine);

    info!("starting node");
    futures::select! {
        _ = api_routine => {
            tracing::info!("event loop shutdown");
        },
        res = coalesced.next() => {
            if let Some(Err(e)) = res {
                if e.is_panic() {
                    panic::resume_unwind(e.into_panic());
                }
            }
        },
        _ = signals_task => {
        }
    }

    if let Err(e) = sockets.cleanup() {
        tracing::error!(err=?e, "error cleaning up sockets");
    }

    Ok(())
}

#[cfg(unix)]
async fn cfg(
    args: &Args,
) -> anyhow::Result<Cfg<discovery::Static, BoxedSigner, request_pull::State>> {
    Ok(Cfg::from_args(args).await?)
}

#[cfg(windows)]
async fn cfg(
    args: &Args,
) -> anyhow::Result<Cfg<discovery::Static, BoxedSigner, request_pull::State>> {
    unimplemented!("Windows is not supported, contributions are welcome :)")
}
