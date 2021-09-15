// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::panic;

use futures::future::{select_all, FutureExt as _};
use structopt::StructOpt as _;
use tokio::{spawn, sync::mpsc};
use tracing::info;

use librad::{
    crypto::BoxedSigner,
    net::{discovery, peer::Peer},
};

use crate::{
    args::Args,
    cfg::{self, Cfg},
    logging,
    metrics::graphite,
    protocol,
    signals,
    tracking,
};

pub async fn run() -> anyhow::Result<()> {
    logging::init();

    let args = Args::from_args();
    let cfg: Cfg<discovery::Static, BoxedSigner> = cfg(&args).await?;

    let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
    let signals_task = tokio::spawn(signals::routine(shutdown_tx));

    let mut coalesced = vec![];
    let peer = Peer::new(cfg.peer)?;
    let peer_task = spawn(protocol::routine(peer.clone(), cfg.disco, shutdown_rx)).fuse();
    coalesced.push(peer_task);

    if let Some(cfg::Metrics::Graphite(addr)) = cfg.metrics {
        let graphite_task = spawn(graphite::routine(peer.clone(), addr)).fuse();
        coalesced.push(graphite_task);
    }

    if let Some(tracker) = cfg.tracker {
        let tracking_task = spawn(tracking::routine(peer.clone(), tracker)).fuse();
        coalesced.push(tracking_task);
    }

    // if let Some(_listener) = socket_activation::env()? {
    // TODO(xla): Schedule listen loop.
    // } else {
    // TODO(xla): Bind to configured/default socket path, constructed from
    // profile info.
    // TODO(xla): Schedule listen loop.
    // }

    // TODO(xla): Setup subroutines.
    //  - Public API
    //  - Anncouncemnets
    //  - Replication Requests
    //  - Tracking

    info!("starting node");
    let (res, _idx, _rest) = select_all(coalesced).await;

    if let Err(e) = res {
        if e.is_panic() {
            panic::resume_unwind(e.into_panic());
        }
    }

    signals_task.await??;

    Ok(())
}

#[cfg(unix)]
async fn cfg(args: &Args) -> anyhow::Result<Cfg<discovery::Static, BoxedSigner>> {
    Ok(Cfg::from_args::<tokio::net::UnixStream>(args).await?)
}

#[cfg(windows)]
async fn cfg(args: &Args) -> anyhow::Result<Cfg<discovery::Static, BoxedSigner>> {
    Ok(Cfg::from_args::<tokio::net::TcpStream>(args).await?)
}
