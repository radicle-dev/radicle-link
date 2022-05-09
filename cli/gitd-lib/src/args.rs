// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{net::SocketAddr, path::PathBuf, str::FromStr, sync::Arc, time::Duration};

use clap::Parser;

use librad::{
    crypto::BoxedSigner,
    profile::{LnkHome, Profile},
};

use crate::{
    config::{self, Config},
    hooks,
};

#[derive(Debug, Parser)]
pub struct Args {
    /// The path to use `LNK_HOME`.
    pub lnk_home: PathBuf,
    #[clap(short)]
    /// The socket address to start the gitd server on.
    pub addr: Option<SocketAddr>,
    #[clap(long)]
    /// The time (in milliseconds) that the gitd server should stay
    /// alive for. If it is not set, the server will live
    /// indefinitely.
    pub linger_timeout: Option<LingerTimeout>,
    #[clap(long)]
    /// The linkd RPC socket address to use for any RPC calls.
    pub linkd_rpc_socket: Option<PathBuf>,
    #[clap(long)]
    /// Announce any changes when the gitd server is processing a
    /// `receive-pack`.
    pub announce_on_push: bool,
    #[clap(long)]
    /// Push any changes to configured seeds when the gitd server is processing
    /// a `receive-pack`.
    pub push_seeds: bool,
    #[clap(long)]
    /// Fetch any changes from configured seeds when the gitd server is
    /// processing a `upload-pack`.
    pub fetch_seeds: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Signer(#[from] lnk_clib::keys::ssh::Error),
    #[error(transparent)]
    Profile(#[from] librad::profile::Error),
    #[error("announce_on_push is true but no linkd_rpc_socket specified")]
    AnnounceWithoutRpc,
}

impl Args {
    pub async fn into_config(
        self,
        spawner: Arc<link_async::Spawner>,
    ) -> Result<Config<BoxedSigner>, Error> {
        let home = LnkHome::Root(self.lnk_home);
        let profile = Profile::from_home(&home, None)?;
        let signer = spawner
            .blocking({
                let profile = profile.clone();
                move || lnk_clib::keys::ssh::signer(&profile, lnk_clib::keys::ssh::SshAuthSock::Env)
            })
            .await?;
        let announce = match (self.announce_on_push, self.linkd_rpc_socket) {
            (true, Some(path)) => Ok(Some(hooks::Announce {
                rpc_socket_path: path,
            })),
            (false, _) => Ok(None),
            (true, None) => Err(Error::AnnounceWithoutRpc),
        }?;
        let network = config::Network {
            announce,
            request_pull: self.push_seeds,
            replicate: self.fetch_seeds,
        };
        Ok(Config {
            paths: profile.paths().clone(),
            signer,
            addr: self.addr,
            linger_timeout: self.linger_timeout.map(|l| l.into()),
            network,
        })
    }
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct LingerTimeout(Duration);

impl From<LingerTimeout> for Duration {
    fn from(l: LingerTimeout) -> Self {
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
