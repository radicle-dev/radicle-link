// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

// TODO(xla): Expose discovery args.
// TODO(xla): Expose storage args.
// TODO(xla): Expose logging args.

use std::{fmt, net::SocketAddr, path::PathBuf, str::FromStr, time::Duration};

use clap::Parser;

use librad::{
    git::Urn,
    net::Network,
    profile::{LnkHome, ProfileId},
    PeerId,
};
use lnk_clib::{keys::ssh::SshAuthSock, seed::Seed};

use crate::tracking;

#[derive(Debug, Default, Eq, PartialEq, Parser)]
pub struct Args {
    /// Usage: `--bootstrap <peer1>@<addr1>[,<label1>] --bootstrap
    /// <peer2>@<addr2>[,<label2>]`
    ///
    /// List of bootstrap nodes for initial discovery. If no bootstrap
    /// nodes were provided, we will fall back to configured
    /// nodes. The max number of configured nodes used will be the
    /// maximum allowed peers in the membership parameters (default:
    /// 5).
    #[clap(long = "bootstrap", name = "bootstrap")]
    pub bootstraps: Vec<Seed<String>>,

    /// Identifier of the profile the daemon will run for. This value determines
    /// which monorepo (if existing) on disk will be the backing storage.
    #[clap(long)]
    pub profile_id: Option<ProfileId>,

    /// Home of the profile data, if not provided is read from the environment
    /// and falls back to project dirs.
    #[clap(long, default_value_t, parse(from_str = parse_lnk_home), env = "LNK_HOME")]
    pub lnk_home: LnkHome,

    /// Which unix domain socket to use for connecting to the ssh-agent. The
    /// default will defer to SSH_AUTH_SOCK, otherwise the value given should be
    /// a valid path.
    #[clap(long, default_value_t)]
    pub ssh_auth_sock: SshAuthSock,

    /// Configures the type of signer used to get access to the storage.
    #[clap(long, default_value_t)]
    pub signer: Signer,

    #[clap(flatten)]
    pub key: KeyArgs,

    #[clap(flatten)]
    pub metrics: MetricsArgs,

    #[clap(flatten)]
    pub protocol: ProtocolArgs,

    /// Forces the creation of a temporary root for the local state, should be
    /// used for debug and testing only.
    #[clap(long)]
    pub tmp_root: bool,

    #[clap(flatten)]
    pub tracking: TrackingArgs,

    #[clap(flatten)]
    pub request_pull: RequestPullStorage,

    /// The number of milliseconds to wait after losing all connections before
    /// shutting down the node. If not specified the node will never
    /// shutdown.
    #[clap(long)]
    pub linger_timeout: Option<LingerTimeout>,
}

#[derive(Debug, Eq, PartialEq, Parser)]
pub enum Signer {
    /// Construct signer from a secret key.
    Key,
    /// Connect to ssh-agent for delegated signing.
    SshAgent,
}

impl Default for Signer {
    fn default() -> Self {
        Self::SshAgent
    }
}

impl fmt::Display for Signer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ty = match self {
            Self::Key => "key",
            Self::SshAgent => "ssh-agent",
        };

        write!(f, "{}", ty)
    }
}

impl FromStr for Signer {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "key" => Ok(Self::Key),
            "ssh-agent" => Ok(Self::SshAgent),
            _ => Err(format!("unsupported signer `{}`", input)),
        }
    }
}

#[derive(Debug, Default, Eq, PartialEq, Parser)]
pub struct KeyArgs {
    /// Location of the key file on disk.
    #[clap(
        long = "key-file-path",
        name = "key-file-path",
        parse(from_str),
        required_if_eq("key-source", "file")
    )]
    pub file_path: Option<PathBuf>,
    /// Format of the key input data.
    #[clap(
        long = "key-format",
        name = "key-format",
        default_value_t,
        required_if_eq("signer", "key")
    )]
    pub format: KeyFormat,
    /// Specifies from which source the secret should be read.
    #[clap(
        long = "key-source",
        name = "key-source",
        default_value_t,
        required_if_eq("signer", "key")
    )]
    pub source: KeySource,
}

#[derive(Debug, Eq, PartialEq, Parser)]
pub enum KeyFormat {
    Base64,
    Binary,
}

impl Default for KeyFormat {
    fn default() -> Self {
        Self::Binary
    }
}

impl fmt::Display for KeyFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let source = match self {
            Self::Base64 => "base64",
            Self::Binary => "binary",
        };
        write!(f, "{}", source)
    }
}

impl FromStr for KeyFormat {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "base64" => Ok(Self::Base64),
            "binary" => Ok(Self::Binary),
            _ => Err(format!("unsupported key format `{}`", input)),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Parser)]
pub enum KeySource {
    Ephemeral,
    File,
    Stdin,
}

impl Default for KeySource {
    fn default() -> Self {
        Self::Stdin
    }
}

impl fmt::Display for KeySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> std::fmt::Result {
        let source = match self {
            Self::Ephemeral => "in-memory",
            Self::File => "file",
            Self::Stdin => "stdin",
        };
        write!(f, "{}", source)
    }
}

impl FromStr for KeySource {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "ephemeral" => Ok(Self::Ephemeral),
            "file" => Ok(Self::File),
            "stdin" => Ok(Self::Stdin),
            _ => Err(format!("unsupported key source `{}`", input)),
        }
    }
}

fn parse_lnk_home(src: &str) -> LnkHome {
    match src {
        dirs if dirs == LnkHome::ProjectDirs.to_string() => LnkHome::ProjectDirs,
        _ => LnkHome::Root(PathBuf::from(src)),
    }
}

#[derive(Debug, Eq, PartialEq, Parser)]
pub struct MetricsArgs {
    /// Provider for metrics collection.
    #[clap(long = "metrics-provider", name = "metrics-provider")]
    pub provider: Option<MetricsProvider>,

    /// Address of the graphite collector to send stats to.
    #[clap(
        long,
        default_value = "localhost:2003",
        required_if_eq("metrics-provider", "graphite")
    )]
    pub graphite_addr: String,
}

impl Default for MetricsArgs {
    fn default() -> Self {
        Self {
            provider: None,
            graphite_addr: "localhost:2003".to_string(),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Parser)]
pub enum MetricsProvider {
    Graphite,
}

impl FromStr for MetricsProvider {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "graphite" => Ok(Self::Graphite),
            _ => Err(format!("unsupported key source `{}`", input)),
        }
    }
}

#[derive(Debug, Default, Eq, PartialEq, Parser)]
pub struct ProtocolArgs {
    /// Address to bind to for the protocol to accept connections. Must be
    /// provided, shortcuts for any (0.0.0.0:0) and localhost (127.0.0.1:0)
    /// are valid values.
    #[clap(long = "protocol-listen", name = "protocol-listen")]
    pub listen: ProtocolListen,

    /// Network name to be used during handshake, if 'main' is passed the
    /// default main network is used.
    #[clap(
        long = "protocol-network",
        name = "protocol-network",
        default_value_t,
        parse(try_from_str = parse_protocol_network))
    ]
    pub network: Network,
    // TODO(xla): Expose protocol args (membership, replication, etc.).
}

#[derive(Debug, Eq, PartialEq, Parser)]
pub enum ProtocolListen {
    Any,
    Localhost,
    Provided { addr: SocketAddr },
}

impl Default for ProtocolListen {
    fn default() -> Self {
        Self::Localhost
    }
}

impl FromStr for ProtocolListen {
    type Err = String;

    fn from_str(src: &str) -> Result<Self, Self::Err> {
        match src {
            "any" => Ok(Self::Any),
            "localhost" => Ok(Self::Localhost),
            addr if !addr.is_empty() => Ok(Self::Provided {
                addr: SocketAddr::from_str(addr).map_err(|err| err.to_string())?,
            }),
            _ => Err("protocol listen must be set".to_string()),
        }
    }
}

fn parse_protocol_network(src: &str) -> Result<Network, String> {
    match src {
        _main if src.to_lowercase() == "main" => Ok(Network::Main),
        custom if !src.is_empty() => Ok(Network::from_str(custom)?),
        _ => Err("custom network can't be empty".to_string()),
    }
}

#[derive(Debug, Default, Eq, PartialEq, Parser)]
pub struct TrackingArgs {
    /// Instruct the node to automatically track either everything it observes
    /// or a selected set of peer ids and urns which need to be provided
    /// through extra arguments to take effect.
    #[clap(long = "track", name = "track")]
    pub mode: Option<TrackingMode>,

    /// Track all updates from a specific peer. Argument can be repeated. Use in
    /// conjunction with `--track="selected"`.
    #[clap(long = "track-peer-id", name = "track-peer-id")]
    pub peer_ids: Vec<PeerId>,

    /// Track all updates for a specific project urn. Argument can be repeated.
    /// Use in conjunction with `--track="selected"`.
    #[clap(long = "track-urn", name = "track-urn")]
    pub urns: Vec<Urn>,

    /// Track all updates for a peer and urn pair, ie. '<peer>,<urn>'. Argument
    /// can be repeated. Use in conjunction with `--track="selected"`.
    ///
    /// Note: if a `track-urn` or `track-peer-id` overlaps with a
    /// `track-pair`, the `track-pair` will take preferrence.
    #[clap(long = "track-pair", name = "track-pair")]
    pub pairs: Vec<tracking::Pair>,
}

#[derive(Debug, Eq, PartialEq, Parser)]
pub enum TrackingMode {
    Everything,
    Selected,
}

impl FromStr for TrackingMode {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "everything" => Ok(Self::Everything),
            "selected" => Ok(Self::Selected),
            _ => Err(format!("unsupported tracking mode `{}`", input)),
        }
    }
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct LingerTimeout(Duration);

impl From<&LingerTimeout> for Duration {
    fn from(l: &LingerTimeout) -> Self {
        l.0
    }
}

impl FromStr for LingerTimeout {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let integer: Result<u64, _> = s.parse();
        match integer {
            Ok(i) => Ok(LingerTimeout(Duration::from_millis(i))),
            Err(_) => Err("expected a positive integer"),
        }
    }
}

/// Settings for the request-pull storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Parser)]
pub struct RequestPullStorage {
    /// Number of [`librad::git::storage::Storage`] instances to reserve.
    #[clap(long = "request-pull-pool-size", default_value_t = num_cpus::get_physical())]
    pub pool_size: usize,
}

impl Default for RequestPullStorage {
    fn default() -> Self {
        Self {
            pool_size: num_cpus::get_physical(),
        }
    }
}
