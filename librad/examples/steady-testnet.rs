// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use argh::FromArgs;
use futures::future;
use librad_test::rad::testnet;

mod common;
use common::logging;

#[derive(FromArgs)]
#[argh(description = "steady-testnet")]
struct Options {
    #[argh(option, description = "number of peers to spawn")]
    num_peers: usize,
}

#[tokio::main]
async fn main() {
    logging::init();
    let Options { num_peers } = argh::from_env();

    let peers = testnet::setup(num_peers).await.unwrap();
    testnet::run_on_testnet(peers, 2, |x| async move {
        let _dontdrop = x.into_iter().map(|(api, _)| api).collect::<Vec<_>>();
        future::pending().await
    })
    .await
}
