// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#![feature(never_type)]

use std::{
    net::{SocketAddr, ToSocketAddrs},
    panic,
    process,
    sync::{
        atomic::{AtomicBool, Ordering::SeqCst},
        Arc,
    },
    thread,
    time::{Duration, SystemTime},
};

use argh::FromArgs;
use futures::FutureExt as _;
use librad::{
    crypto,
    git,
    net::{
        discovery::{self, Discovery as _},
        peer::{self, Peer},
        protocol::{self, io},
        Network,
    },
    paths::Paths,
    PeerId,
    SecretKey,
};
use radicle_link_e2e::logging;
use tempfile::tempdir;
use tokio::task::JoinError;

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
                .map_err(|e: crypto::peer::conversion::Error| e.to_string())?;
            Ok(BoostrapNode {
                peer_id,
                addr: addr.to_owned(),
            })
        },

        None => Err("missing peer id".to_owned()),
    }
}

fn parse_secret_key(s: &str) -> Result<SecretKey, String> {
    use librad::crypto::keystore::SecretKeyExt as _;

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
                rate_limits: Default::default(),
            },
            storage: Default::default(),
        })
        .unwrap();
        let bound = peer.bind().await.unwrap();
        let disco = discovery::Static::resolve(
            opts.bootstrap
                .into_iter()
                .map(|BoostrapNode { peer_id, addr }| (peer_id, addr)),
        )
        .unwrap();

        let (term, run) = bound.accept(disco.discover());
        install_signal_handlers(term);

        let mut protocol = tokio::spawn(run).fuse();
        let mut metrics = match opts.graphite {
            None => tokio::spawn(stdout_stats(peer)),
            Some(addr) => {
                let addr = addr.to_socket_addrs().unwrap().next().unwrap();
                tokio::spawn(graphite_stats(peer, addr))
            },
        }
        .fuse();

        let res = futures::select! {
            p = protocol => p.map(|inner| inner.map(|_| ()).or_else(|e| match e {
                io::error::Accept::Done => Ok(()),
                _ => Err(e.into())
            })),
            m = metrics => m.map(|inner| inner.map(|_| ())),
        };
        handle_shutdown(res)
    }
}

fn handle_shutdown(r: Result<anyhow::Result<()>, JoinError>) {
    let res = match r {
        Err(e) if e.is_panic() => panic::resume_unwind(e.into_panic()),
        Ok(Err(e)) => Some(e),
        _ => None,
    };

    process::exit(match res {
        Some(e) => {
            eprintln!("FATAL: {}", e);
            1
        },
        None => 0,
    })
}

fn install_signal_handlers<F>(term: F)
where
    F: FnOnce() + Send + 'static,
{
    use signal_hook::{
        consts::TERM_SIGNALS,
        flag::register_conditional_shutdown,
        low_level::register,
    };

    let stop = Arc::new(AtomicBool::new(false));
    let sig_handler = thread::spawn({
        let stop = Arc::clone(&stop);
        move || loop {
            if stop.load(SeqCst) {
                term();
                break;
            }

            thread::park();
        }
    });

    for sig in TERM_SIGNALS {
        register_conditional_shutdown(*sig, 1, Arc::clone(&stop)).unwrap();
        unsafe {
            let stop = Arc::clone(&stop);
            let thread = sig_handler.thread().clone();
            register(*sig, move || {
                stop.store(true, SeqCst);
                thread.unpark()
            })
            .unwrap();
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
