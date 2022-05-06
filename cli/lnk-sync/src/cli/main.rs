// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use tokio::runtime::Runtime;

use librad::{
    net::{
        self,
        peer::{client, Client},
        quic,
        Network,
    },
    profile::{LnkHome, Profile, ProfileId},
};
use link_async::Spawner;
use lnk_clib::{
    keys::ssh::{self, SshAuthSock},
    seed::{self, Seeds},
};

use crate::{cli::args::Args, sync};

pub fn main(
    args: Args,
    profile: Option<ProfileId>,
    sock: SshAuthSock,
    runtime: Runtime,
) -> anyhow::Result<()> {
    let home = LnkHome::default();
    let profile = Profile::from_home(&home, profile)?;

    runtime.block_on(async move {
        let paths = profile.paths();
        let spawner = Arc::new(Spawner::from_current().unwrap());
        let signer = spawner
            .blocking({
                let profile = profile.clone();
                move || ssh::signer(&profile, sock)
            })
            .await?;

        let config = client::Config {
            signer: signer.clone(),
            paths: paths.clone(),
            replication: net::replication::Config::default(),
            user_storage: client::config::Storage::default(),
            network: Network::default(),
        };
        let endpoint = quic::SendOnly::new(signer, Network::default()).await?;
        let client = Client::new(config, spawner, endpoint)?;
        let seeds = {
            let seeds_file = profile.paths().seeds_file();
            let store = seed::store::FileStore::<String>::new(seeds_file)?;
            let (seeds, errors) = Seeds::load(&store, None).await?;

            for error in errors {
                eprintln!("failed to load seed: {}", error);
                tracing::warn!(error = %error, "failed to load seed")
            }

            if seeds.is_empty() {
                eprintln!(
                    "no seeds were found to sync with in {}",
                    seeds_file.display()
                );
                return Ok(());
            }

            seeds
        };
        let synced = sync(&client, args.urn, seeds, args.mode).await;
        println!("{}", serde_json::to_string(&synced)?);
        Ok(())
    })
}
