// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    convert::TryFrom,
    io,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, ToSocketAddrs as _},
    time::Duration,
};

use anyhow::{bail, Context, Result};
use tokio::{
    fs::File,
    io::{stdin, AsyncReadExt as _},
    time::{error::Elapsed, timeout},
};
use tracing::warn;

use librad::{
    crypto::{BoxedSigner, IntoSecretKeyError},
    git::storage,
    keystore::SecretKeyExt as _,
    net,
    net::{discovery, peer::Config as PeerConfig, protocol::membership},
    paths,
    profile::{LnkHome, Profile},
    SecretKey,
};
use lnk_clib::keys;

use crate::{args, tracking::Tracker};

use crate::seed::{self, store::FileStore, Seeds};

lazy_static::lazy_static! {
    /// General binding to any available port, i.e. `0.0.0.0:0`.
    pub static ref ANY: SocketAddr =
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0));

    /// Localhost binding to any available port, i.e. `127.0.0.1:0`.
    pub static ref LOCALHOST: SocketAddr =
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 0));
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("decoding base64 key")]
    Base64(#[from] base64::DecodeError),

    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    Init(#[from] storage::error::Init),

    #[error(transparent)]
    Keys(#[from] keys::ssh::Error),

    #[error("no bootstrap nodes could be resolved")]
    NoBootstrap,

    #[error("no seed nodes could be resolved")]
    NoSeeds,

    #[error(transparent)]
    Other(#[from] anyhow::Error),

    #[error(transparent)]
    Profile(#[from] librad::profile::Error),

    #[error(transparent)]
    SecretKey(#[from] IntoSecretKeyError),

    #[error(transparent)]
    Seed(#[from] seed::error::Load),

    #[error(transparent)]
    Timeout(#[from] Elapsed),
}

pub enum RunMode {
    Immortal,
    Mortal(Duration),
}

impl Default for RunMode {
    fn default() -> Self {
        RunMode::Immortal
    }
}

pub struct Cfg<Disco, Signer> {
    pub disco: Disco,
    pub metrics: Option<Metrics>,
    pub peer: PeerConfig<Signer>,
    pub tracker: Option<Tracker>,
    pub run_mode: RunMode,
    pub profile: Profile,
}

impl Cfg<discovery::Static, BoxedSigner> {
    pub async fn from_args(args: &args::Args) -> Result<Self, Error> {
        let membership = membership::Params::default();
        let seeds = if !args.bootstraps.is_empty() {
            let (seeds, failures) = Seeds::resolve(args.bootstraps.iter()).await;
            for fail in failures {
                tracing::warn!("failed to load bootstrap seed: {}", fail);
            }

            if seeds.is_empty() {
                return Err(Error::NoBootstrap);
            }

            seeds
        } else {
            let store = FileStore::<String>::new(paths::seeds()?)?;
            let (seeds, failures) = Seeds::load(&store, membership.max_active).await?;

            for fail in failures {
                tracing::warn!("failed to load configured seed: {}", fail)
            }

            if seeds.is_empty() {
                return Err(Error::NoSeeds);
            }

            seeds
        };
        let disco = discovery::Static::try_from(seeds)?;
        let profile = Profile::try_from(args)?;
        let signer = construct_signer(args, &profile).await?;

        // Ensure the storage is accessible for the created profile and signer.
        storage::Storage::init(profile.paths(), signer.clone())?;

        let listen_addr = match args.protocol.listen {
            args::ProtocolListen::Any => *ANY,
            args::ProtocolListen::Localhost => *LOCALHOST,
            args::ProtocolListen::Provided { addr } => addr,
        };

        let metrics = match args.metrics.provider {
            Some(args::MetricsProvider::Graphite) => Some(Metrics::Graphite(
                args.metrics
                    .graphite_addr
                    .to_socket_addrs()?
                    .next()
                    .unwrap(),
            )),
            None => None,
        };

        let run_mode = match &args.linger_timeout {
            Some(t) => RunMode::Mortal(t.into()),
            None => RunMode::Immortal,
        };

        Ok(Self {
            disco,
            metrics,
            peer: PeerConfig {
                signer,
                protocol: net::protocol::Config {
                    paths: profile.paths().clone(),
                    listen_addr,
                    advertised_addrs: None,
                    membership,
                    network: args.protocol.network.clone(),
                    replication: Default::default(),
                    rate_limits: Default::default(),
                },
                storage: Default::default(),
            },
            tracker: args.tracking.mode.as_ref().map(|arg| match arg {
                args::TrackingMode::Everything => Tracker::Everything,
                args::TrackingMode::Selected => Tracker::Selected {
                    peer_ids: args.tracking.peer_ids.clone().into_iter().collect(),
                    urns: args.tracking.urns.clone().into_iter().collect(),
                },
            }),
            profile,
            run_mode,
        })
    }
}

pub enum Metrics {
    Graphite(SocketAddr),
}

impl TryFrom<&args::Args> for Profile {
    type Error = Error;

    fn try_from(args: &args::Args) -> Result<Self, Self::Error> {
        let home = if args.tmp_root {
            warn!("creating temporary root which is ephemeral and should only be used for debug and testing");
            LnkHome::Root(tempfile::tempdir()?.path().to_path_buf())
        } else {
            args.lnk_home.clone()
        };

        Profile::from_home(&home, args.profile_id.clone()).map_err(Error::from)
    }
}

async fn construct_signer(args: &args::Args, profile: &Profile) -> anyhow::Result<BoxedSigner> {
    match args.signer {
        args::Signer::SshAgent => {
            tokio::task::spawn_blocking({
                let profile = profile.clone();
                let sock = args.ssh_auth_sock.clone();
                move || keys::ssh::signer(&profile, sock).map_err(anyhow::Error::from)
            })
            .await?
        },
        args::Signer::Key => {
            let bytes = match args.key.source {
                args::KeySource::Ephemeral => {
                    warn!("generating key in-memory which is ephemeral and should only be used for debug and testing");

                    SecretKey::new().as_ref().to_vec()
                },
                args::KeySource::File => {
                    if args.key.file_path.is_none() {
                        bail!("file path must be present when file source is set");
                    }

                    let mut file = File::open(args.key.file_path.clone().unwrap())
                        .await
                        .context("opening key file")?;
                    let mut bytes = vec![];

                    timeout(Duration::from_secs(5), file.read_to_end(&mut bytes))
                        .await?
                        .context("reading key file")?;

                    bytes
                },
                args::KeySource::Stdin => {
                    let mut bytes = vec![];
                    timeout(Duration::from_secs(5), stdin().read_to_end(&mut bytes))
                        .await?
                        .context("reading stdin")?;
                    bytes
                },
            };

            let key = match args.key.format {
                args::KeyFormat::Base64 => {
                    let bs = base64::decode(&bytes)?;
                    SecretKey::from_bytes_and_meta(bs.into(), &())?
                },
                args::KeyFormat::Binary => SecretKey::from_bytes_and_meta(bytes.into(), &())?,
            };

            Ok(BoxedSigner::from(key))
        },
    }
}
