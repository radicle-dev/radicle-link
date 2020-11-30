// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::net::SocketAddr;

use anyhow::anyhow;
use argh::FromArgs;
use futures::future;
use librad::PeerId;
use librad_test::rad::testnet;

mod common;
use common::logging;

#[derive(FromArgs)]
#[argh(description = "steady-peer")]
struct Options {
    #[argh(option, description = "boostrap nodes")]
    bootstrap: Vec<String>,
}

#[tokio::main]
async fn main() {
    logging::init();
    run(argh::from_env()).await.unwrap()
}

async fn run(Options { bootstrap }: Options) -> anyhow::Result<()> {
    let bootstrap = bootstrap
        .iter()
        .map(parse_bootstrap)
        .collect::<Result<Vec<_>, anyhow::Error>>()?;

    let testnet::TestPeer { _tmp, peer, key: _ } = testnet::boot(bootstrap).await?;
    let (_api, run) = peer.accept()?;
    future::join(run, future::pending::<()>()).await;

    Ok(())
}

fn parse_bootstrap<S: AsRef<str>>(s: S) -> anyhow::Result<(PeerId, SocketAddr)> {
    match s.as_ref().split(',').collect::<Vec<_>>().as_slice() {
        [peerid, addr, ..] => {
            let peerid = peerid.parse()?;
            let addr = addr.parse()?;
            Ok((peerid, addr))
        },

        _ => Err(anyhow!("couldn't parse {}", s.as_ref())),
    }
}
