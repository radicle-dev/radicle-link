// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#![feature(never_type)]

use std::{
    net::{SocketAddr, ToSocketAddrs},
    panic,
    time::{Duration, SystemTime},
};

use argh::FromArgs;
use futures::future;
use librad::{
    git,
    keys::SecretKey,
    net::{
        discovery::{self, Discovery as _},
        peer::{self, Peer},
        protocol,
        Network,
    },
    paths::Paths,
    PeerId,
};
use radicle_link_e2e::logging;
use tempfile::tempdir;

/// A passive peer using temporary storage
#[derive(FromArgs)]
struct Options {
    /// the network to join
    #[argh(option, default = "Network::Custom(b\"localtestnet\".as_ref().into())")]
    network: Network,
    /// base64-encoded secret key. A random key is generated if empty.
    #[argh(option, from_str_fn(parse_secret_key))]
    secret_key: Option<SecretKey>,
    /// listen address.
    #[argh(option)]
    listen: Option<SocketAddr>,
    /// addresses of peers to use as bootstrap nodes.
    #[argh(option, from_str_fn(parse_bootstrap_node))]
    bootstrap: Vec<BoostrapNode>,
    /// graphite address.
    #[argh(option)]
    graphite: Option<String>,
}

#[derive(Debug)]
struct BoostrapNode {
    peer_id: PeerId,
    addr: String,
}

fn parse_bootstrap_node(s: &str) -> Result<BoostrapNode, String> {
    match s.split_once('@') {
        Some((peer_id, addr)) => {
            let peer_id = peer_id
                .parse()
                .map_err(|e: librad::peer::conversion::Error| e.to_string())?;
            Ok(BoostrapNode {
                peer_id,
                addr: addr.to_owned(),
            })
        },

        None => Err("missing peer id".to_owned()),
    }
}

fn parse_secret_key(s: &str) -> Result<SecretKey, String> {
    use radicle_keystore::SecretKeyExt as _;

    base64::decode(s)
        .map_err(|e| e.to_string())
        .and_then(|bs| SecretKey::from_bytes_and_meta(bs.into(), &()).map_err(|e| e.to_string()))
}

#[tokio::main]
async fn main() {
    logging::init();

    let opts: Options = argh::from_env();
    tracing::info!("listen: {:?}", opts.listen);
    tracing::info!("bootstrap: {:?}", opts.bootstrap);

    let root = tempdir().unwrap();
    {
        let paths = Paths::from_root(root.path()).unwrap();
        let key = opts.secret_key.unwrap_or_else(SecretKey::new);

        // eagerly init so we crash immediately on error
        git::storage::Storage::init(&paths, key.clone()).unwrap();

        let peer = Peer::new(peer::Config {
            signer: key,
            protocol: protocol::Config {
                paths,
                listen_addr: opts.listen.unwrap_or_else(|| "0.0.0.0:0".parse().unwrap()),
                advertised_addrs: None,
                membership: Default::default(),
                network: opts.network,
                replication: Default::default(),
                fetch: Default::default(),
                graft: Default::default(),
            },
            storage: Default::default(),
        });
        let bound = peer.bind().await.unwrap();
        let disco = discovery::Static::resolve(
            opts.bootstrap
                .into_iter()
                .map(|BoostrapNode { peer_id, addr }| (peer_id, addr)),
        )
        .unwrap();

        let protocol = tokio::spawn(bound.accept(disco.discover()));
        let metrics = match opts.graphite {
            None => tokio::spawn(stdout_stats(peer)),
            Some(addr) => {
                let addr = addr.to_socket_addrs().unwrap().next().unwrap();
                tokio::spawn(graphite_stats(peer, addr))
            },
        };
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

async fn stdout_stats(peer: Peer<SecretKey>) -> anyhow::Result<!> {
    loop {
        tokio::time::sleep(Duration::from_secs(10)).await;
        let stats = peer.stats().await;
        tracing::info!("{}: {:?}", peer.peer_id(), stats);
    }
}

async fn graphite_stats(peer: Peer<SecretKey>, graphite_addr: SocketAddr) -> anyhow::Result<!> {
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

    tracing::info!("connecting to graphite at {}", graphite_addr);
    let sock = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
    sock.connect(graphite_addr).await?;
    tracing::info!("connected to graphite at {}", graphite_addr);
    loop {
        tokio::time::sleep(Duration::from_secs(10)).await;
        let stats = peer.stats().await;
        tracing::info!("{}: {:?}", peer.peer_id(), stats);

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap();

        for (metric, value) in &[
            (CONNECTIONS_TOTAL, stats.connections_total),
            (CONNECTED_PEERS, stats.connected_peers.len()),
            (MEMBERSHIP_ACTIVE, stats.membership_active),
            (MEMBERSHIP_PASSIVE, stats.membership_passive),
        ] {
            sock.send(graphite_line(now, metric, *value as f32).as_bytes())
                .await?;
        }
    }
}
