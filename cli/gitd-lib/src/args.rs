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

#[derive(Debug, Parser)]
pub struct Args {
    pub lnk_home: PathBuf,
    #[clap(short)]
    pub addr: Option<SocketAddr>,
    #[clap(long)]
    pub linger_timeout: Option<LingerTimeout>,
    #[clap(long)]
    pub linkd_rpc_socket: Option<PathBuf>,
    #[clap(long)]
    pub announce_on_push: bool,
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
    ) -> Result<super::config::Config<BoxedSigner>, Error> {
        let home = LnkHome::Root(self.lnk_home);
        let profile = Profile::from_home(&home, None)?;
        let signer = spawner
            .blocking({
                let profile = profile.clone();
                move || lnk_clib::keys::ssh::signer(&profile, lnk_clib::keys::ssh::SshAuthSock::Env)
            })
            .await?;
        let announce = match (self.announce_on_push, self.linkd_rpc_socket) {
            (true, Some(path)) => Ok(Some(super::config::Announce {
                rpc_socket_path: path,
            })),
            (false, _) => Ok(None),
            (true, None) => Err(Error::AnnounceWithoutRpc),
        }?;
        Ok(super::config::Config {
            paths: profile.paths().clone(),
            signer,
            addr: self.addr,
            linger_timeout: self.linger_timeout.map(|l| l.into()),
            announce,
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
