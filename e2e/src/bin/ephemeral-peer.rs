// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#![feature(never_type)]

use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    panic,
    time::{Duration, SystemTime},
};

use futures::{future, StreamExt as _};
use lazy_static::lazy_static;
use librad::{
    git,
    keys::SecretKey,
    net::{
        discovery,
        peer::{self, Peer},
        protocol,
    },
    paths::Paths,
};
use radicle_link_e2e::logging;
use tempfile::tempdir;

lazy_static! {
    static ref LOCALHOST_ANY: SocketAddr =
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 0));
}

#[tokio::main]
async fn main() {
    logging::init();

    let root = tempdir().unwrap();
    {
        let paths = Paths::from_root(root.path()).unwrap();
        let key = SecretKey::new();

        git::storage::Storage::init(&paths, key.clone()).unwrap();

        let peer = Peer::new(peer::Config {
            signer: key,
            protocol: protocol::Config {
                paths,
                listen_addr: *LOCALHOST_ANY,
                membership: Default::default(),
                network: Default::default(),
                replication: Default::default(),
            },
            storage_pools: Default::default(),
        });
        let bound = peer.bind().await.unwrap();
        let disco = discovery::Mdns::new(
            peer.peer_id(),
            bound.listen_addrs().unwrap(),
            Duration::from_secs(60),
            Duration::from_secs(60),
        )
        .unwrap();

        let protocol = tokio::spawn(bound.accept(disco.take(10)));
        let metrics = tokio::spawn(emit_stats(peer));
        match future::try_join(protocol, metrics).await {
            Err(e) => {
                if let Ok(panicked) = e.try_into_panic() {
                    panic::resume_unwind(panicked)
                }
            },

            Ok(res) => match res {
                (Err(e), _) => panic!("protocol error: {:?}", e),
                (_, Err(e)) => panic!("metrics error: {:?}", e),
                _ => {},
            },
        }
    }
}

async fn emit_stats(peer: Peer<SecretKey>) -> anyhow::Result<!> {
    tracing::debug!("stats collector");

    let peer_id_str = peer.peer_id().to_string();

    const CONNECTIONS_TOTAL: &str = "connections_total";
    const CONNECTED_PEERS: &str = "connected_peers";
    const MEMBERSHIP_ACTIVE: &str = "membership_active";
    const MEMBERSHIP_PASSIVE: &str = "membership_passive";

    let graphite_line = |time: Duration, metric: &str, value: f32| -> String {
        format!(
            "link_{};peer={} {:?} {}",
            metric,
            peer_id_str,
            value,
            time.as_secs()
        )
    };

    let sock = tokio::net::UdpSocket::bind("127.0.0.1:0").await?;
    sock.connect("127.0.0.1:9109").await?;
    loop {
        tokio::time::sleep(Duration::from_secs(10)).await;
        let stats = peer.stats().await;
        tracing::info!("stats: {:?}", stats);

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap();

        for (metric, value) in &[
            (CONNECTIONS_TOTAL, stats.connections_total),
            (CONNECTED_PEERS, stats.connected_peers),
            (MEMBERSHIP_ACTIVE, stats.membership_active),
            (MEMBERSHIP_PASSIVE, stats.membership_passive),
        ] {
            sock.send(graphite_line(now, metric, *value as f32).as_bytes())
                .await?;
        }
    }
}
