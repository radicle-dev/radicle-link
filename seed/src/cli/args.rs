// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{net, path::PathBuf};

use argh::FromArgs;

use librad::{git::Urn, net::protocol::membership, peer::PeerId};

/// A set of peers to track
#[derive(FromArgs)]
#[argh(subcommand, name = "track-peers")]
pub struct Peers {
    /// track the specified peer only
    #[argh(option, long = "peer")]
    pub peers: Vec<PeerId>,
}

/// A set of URNs to track
#[derive(FromArgs)]
#[argh(subcommand, name = "track-urns")]
pub struct Urns {
    /// track the specified URN only
    #[argh(option, long = "urn")]
    pub urns: Vec<Urn>,
}

#[derive(FromArgs)]
#[argh(subcommand)]
pub enum Track {
    Urns(Urns),
    Peers(Peers),
}

#[derive(FromArgs)]
/// Radicle Seed.
pub struct Options {
    /// track the specified peer only
    #[argh(subcommand)]
    pub track: Option<Track>,

    /// listen on the following address for peer connections
    #[argh(option)]
    pub peer_listen: Option<net::SocketAddr>,

    /// listen on the following address for HTTP connections (default:
    /// 127.0.0.1:8888)
    #[argh(option, default = "std::net::SocketAddr::from(([127, 0, 0, 1], 8888))")]
    pub http_listen: net::SocketAddr,

    /// log level (default: info)
    #[argh(option, default = "tracing::Level::INFO")]
    pub log: tracing::Level,

    /// radicle root path, for key and git storage
    #[argh(option)]
    pub root: Option<PathBuf>,

    /// path to UI assets directory
    #[argh(option, default = "PathBuf::from(\"ui/public\")")]
    pub assets_path: PathBuf,

    /// name of this seed, displayed to users
    #[argh(option)]
    pub name: Option<String>,

    /// description of this seed, displayed to users as HTML
    #[argh(option)]
    pub description: Option<String>,

    /// public address of this seed node, eg. 'seedling.radicle.xyz:12345'
    #[argh(option)]
    pub public_addr: Option<String>,

    /// list of bootstrap peers, eg.
    /// 'f00...@seed1.example.com:12345,bad...@seed2.example.com:12345'
    #[argh(option)]
    pub bootstrap: Option<String>,

    /// number of [`librad::git::storage::Storage`] instancess to pool for
    /// consumers.
    #[argh(option, default = "num_cpus::get_physical()")]
    pub user_size: usize,

    /// number of [`librad::git::storage::Storage`] instancess to pool for the
    /// protocol.
    #[argh(option, default = "num_cpus::get_physical()")]
    pub protocol_size: usize,

    /// max number of active members to set in [`membership::Params`].
    #[argh(option, default = "membership::Params::default().max_active")]
    pub membership_max_active: usize,

    /// max number of passive members to set in [`membership::Params`].
    #[argh(option, default = "membership::Params::default().max_passive")]
    pub membership_max_passive: usize,
}

impl Options {
    pub fn from_env() -> Self {
        argh::from_env()
    }
}
