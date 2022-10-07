// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use lnk_identities::working_copy_dir::WorkingCopyDir;
use tokio::runtime::Runtime;

use librad::{
    git::{identities::project::heads, storage::ReadOnlyStorage},
    net::{
        self,
        peer::{client, Client},
        quic, Network,
    },
    profile::{LnkHome, Profile, ProfileId},
};
use link_async::Spawner;
use lnk_clib::{
    keys::ssh::{self, SshAuthSock},
    seed::{self, Seeds},
};

use crate::{cli::args::Args, forked, sync};

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
        let endpoint = quic::SendOnly::new(signer.clone(), Network::default()).await?;
        let client = Client::new(config, spawner, endpoint)?;
        let seeds = {
            let seeds_file = profile.paths().seeds_file();
            let store = seed::store::FileStore::<String>::new(seeds_file)?;
            let (seeds, errors) = Seeds::load(&store, None)?;

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
        match args {
            Args::Sync { urn, mode } => {
                let synced = sync(&client, urn, seeds, mode).await;
                println!("{}", serde_json::to_string(&synced)?);
            },
            Args::Clone { urn, path, peer } => {
                let storage = librad::git::Storage::open(paths, signer.clone())?;

                let already_had_urn = storage.has_urn(&urn)?;
                let path = WorkingCopyDir::at_or_current_dir(path)?;
                println!("cloning urn {} into {}", urn, path);
                println!("syncing monorepo with seeds");
                sync(&client, urn.clone(), seeds, crate::Mode::Fetch).await;

                if !already_had_urn {
                    // This is the first time we've seen this project, so we set the default head

                    let vp = librad::git::identities::project::verify(&storage, &urn)?
                        .ok_or_else(|| anyhow::anyhow!("no such project"))?;

                    if peer.is_none() {
                        match heads::set_default_head(&storage, vp) {
                            Ok(_) => {},
                            Err(heads::error::SetDefaultBranch::Forked(forks)) => {
                                let error = forked::ForkError::from_forked(&storage, forks);
                                println!("{}", error);
                                return Ok(());
                            },
                            Err(e) => anyhow::bail!("error setting HEAD for project: {}", e),
                        }
                    }
                }
                let repo = lnk_identities::project::checkout(
                    &storage,
                    paths.clone(),
                    signer,
                    &urn,
                    peer,
                    path,
                )?;
                println!("working copy created at `{}`", repo.path().display());
            },
        }
        Ok(())
    })
}
