// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    env,
    net::{SocketAddr, ToSocketAddrs},
    str::FromStr,
};

use tracing_subscriber::{EnvFilter, FmtSubscriber};

use librad::{
    git::replication,
    net::{
        peer,
        protocol::{self, membership},
        Network,
    },
    paths,
    peer::PeerId,
    profile,
};

use crate::{cli::args, frontend, Mode, Node, NodeConfig, Signer};

fn parse_peer_address(address: &str) -> SocketAddr {
    address
        .to_socket_addrs()
        .map(|mut a| a.next())
        .expect("peer address could not be parsed")
        .expect("peer address could not be resolved")
}

fn parse_peer_list(option: String) -> Vec<(PeerId, SocketAddr)> {
    option
        .split(',')
        .map(|entry| entry.splitn(2, '@').collect())
        .into_iter()
        .map(|parts: Vec<&str>| {
            (
                PeerId::from_str(parts[0]).expect("peer id could not be parsed"),
                parse_peer_address(parts[1]),
            )
        })
        .collect()
}

pub async fn main() {
    let opts = args::Options::from_env();
    let subscriber = FmtSubscriber::builder();
    if env::var("RUST_LOG").is_ok() {
        let subscriber = subscriber
            .with_env_filter(EnvFilter::from_default_env())
            .finish();

        tracing::subscriber::set_global_default(subscriber)
            .expect("setting tracing subscriber should succeed");
    } else {
        let subscriber = subscriber.with_max_level(opts.log).finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("setting tracing subscriber should succeed");
    };

    let signer = match Signer::new(std::io::stdin()) {
        Ok(signer) => signer,
        Err(err) => panic!("invalid key was supplied to stdin: {}", err),
    };
    let paths = if let Some(root) = &opts.root {
        paths::Paths::from_root(root).expect("failed to configure paths")
    } else {
        profile::Profile::load()
            .expect("failed to load profile")
            .paths()
            .to_owned()
    };

    let storage = peer::config::Storage {
        user: peer::config::UserStorage {
            pool_size: opts.user_size,
        },
        protocol: peer::config::ProtocolStorage {
            pool_size: opts.protocol_size,
            ..peer::config::ProtocolStorage::default()
        },
    };
    let membership = membership::Params {
        max_active: opts.membership_max_active,
        max_passive: opts.membership_max_passive,
        ..membership::Params::default()
    };
    let listen_addr = opts.peer_listen.unwrap_or_else(|| ([0, 0, 0, 0], 0).into());

    let config = NodeConfig {
        mode: match opts.track {
            Some(args::Track::Peers(args::Peers { peers })) => {
                Mode::TrackPeers(peers.into_iter().collect())
            },
            Some(args::Track::Urns(args::Urns { urns })) => {
                Mode::TrackUrns(urns.into_iter().collect())
            },
            None => Mode::TrackEverything,
        },
        bootstrap: opts.bootstrap.map_or_else(Vec::new, parse_peer_list),
    };
    let peer_config = peer::Config {
        signer: signer.clone(),
        protocol: protocol::Config {
            paths,
            listen_addr,
            advertised_addrs: None,
            membership,
            network: Network::default(),
            replication: replication::Config::default(),
            fetch: protocol::config::Fetch::default(),
        },
        storage,
    };
    let node = Node::new().unwrap();
    let handle = node.handle();
    let peer_id = PeerId::from(signer);
    let (tx, rx) = futures::channel::mpsc::channel(1);

    tokio::spawn(frontend::run(
        opts.name,
        opts.description,
        opts.http_listen,
        opts.public_addr,
        opts.assets_path,
        peer_id,
        handle,
        rx,
    ));

    node.run(config, peer_config, tx).await.unwrap();
}
