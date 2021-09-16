// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

// TODO(xla): Expose discovery args.
// TODO(xla): Expose storage args.
// TODO(xla): Expose logging args.

use std::{fmt, net::SocketAddr, path::PathBuf, str::FromStr};

use structopt::StructOpt;

use librad::{
    crypto,
    net::Network,
    profile::{ProfileId, RadHome},
    PeerId,
};

#[derive(Debug, Default, Eq, PartialEq, StructOpt)]
pub struct Args {
    /// List of bootstrap nodes for initial discovery.
    #[structopt(long = "bootstrap", name = "bootstrap")]
    pub bootstraps: Vec<Bootstrap>,

    /// Identifier of the profile the daemon will run for. This value determines
    /// which monorepo (if existing) on disk will be the backing storage.
    #[structopt(long)]
    pub profile_id: Option<ProfileId>,

    /// Home of the profile data, if not provided is read from the environment
    /// and falls back to project dirs.
    #[structopt(long, default_value, parse(from_str = parse_rad_home))]
    pub rad_home: RadHome,

    /// Configures the type of signer used to get access to the storage.
    #[structopt(long, default_value)]
    pub signer: Signer,

    #[structopt(flatten)]
    pub key: KeyArgs,

    #[structopt(flatten)]
    pub metrics: MetricsArgs,

    #[structopt(flatten)]
    pub protocol: ProtocolArgs,

    /// Forces the creation of a temporary root for the local state, should be
    /// used for debug and testing only.
    #[structopt(long)]
    pub tmp_root: bool,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Bootstrap {
    pub addr: String,
    pub peer_id: PeerId,
}

impl fmt::Display for Bootstrap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.peer_id, self.addr)
    }
}

impl FromStr for Bootstrap {
    type Err = String;

    fn from_str(src: &str) -> Result<Self, Self::Err> {
        match src.split_once('@') {
            Some((peer_id, addr)) => {
                let peer_id = peer_id
                    .parse()
                    .map_err(|e: crypto::peer::conversion::Error| e.to_string())?;

                Ok(Self {
                    addr: addr.to_string(),
                    peer_id,
                })
            },
            None => Err("missing peer id".to_string()),
        }
    }
}

#[derive(Debug, Eq, PartialEq, StructOpt)]
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

#[derive(Debug, Default, Eq, PartialEq, StructOpt)]
pub struct KeyArgs {
    /// Location of the key file on disk.
    #[structopt(
        long = "key-file-path",
        name = "key-file-path",
        parse(from_str),
        required_if("key-source", "file")
    )]
    pub file_path: Option<PathBuf>,
    /// Format of the key input data.
    #[structopt(
        long = "key-format",
        name = "key-format",
        default_value,
        required_if("signer", "key")
    )]
    pub format: KeyFormat,
    /// Specifies from which source the secret should be read.
    #[structopt(
        long = "key-source",
        name = "key-source",
        default_value,
        required_if("signer", "key")
    )]
    pub source: KeySource,
}

#[derive(Debug, Eq, PartialEq, StructOpt)]
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

#[derive(Debug, Eq, PartialEq, StructOpt)]
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

fn parse_rad_home(src: &str) -> RadHome {
    match src {
        dirs if dirs == RadHome::ProjectDirs.to_string() => RadHome::ProjectDirs,
        _ => RadHome::Root(PathBuf::from(src)),
    }
}

#[derive(Debug, Eq, PartialEq, StructOpt)]
pub struct MetricsArgs {
    /// Provider for metrics collection.
    #[structopt(long = "metrics-provider", name = "metrics-provider")]
    pub provider: Option<MetricsProvider>,

    /// Address of the graphite collector to send stats to.
    #[structopt(
        long,
        default_value = "localhost:2003",
        required_if("metrics-provider", "graphite")
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

#[derive(Debug, Eq, PartialEq, StructOpt)]
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

#[derive(Debug, Default, Eq, PartialEq, StructOpt)]
pub struct ProtocolArgs {
    /// Address to bind to for the protocol to accept connections. Must be
    /// provided, shortcuts for any (0.0.0.0:0) and localhost (127.0.0.1:0)
    /// are valid values.
    #[structopt(long = "protocol-listen", name = "protocol-listen", parse(try_from_str = ProtocolListen::parse))]
    pub listen: ProtocolListen,

    /// Network name to be used during handshake, if 'main' is passed the
    /// default main network is used.
    #[structopt(
        long = "protocol-network",
        name = "protocol-network",
        default_value,
        parse(try_from_str = parse_protocol_network))
    ]
    pub network: Network,
    // TODO(xla): Expose protocol args (membership, replication, etc.).
}

#[derive(Debug, Eq, PartialEq, StructOpt)]
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

impl ProtocolListen {
    fn parse(src: &str) -> Result<Self, String> {
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
