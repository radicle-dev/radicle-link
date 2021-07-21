// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use thrussh_agent::client::ClientStream;

use librad::profile::Profile;
use rad_clib::storage::ssh;

use crate::{cli::args::tracking::*, tracking};

pub async fn eval_track<S>(profile: &Profile, Track { urn, peer }: Track) -> anyhow::Result<()>
where
    S: ClientStream + Unpin + 'static,
{
    let (_, storage) = ssh::storage::<S>(profile).await?;
    let paths = profile.paths();
    tracking::track(&storage, paths, &urn, peer)?;
    Ok(())
}

pub async fn eval_untrack<S>(
    profile: &Profile,
    Untrack { urn, peer }: Untrack,
) -> anyhow::Result<()>
where
    S: ClientStream + Unpin + 'static,
{
    let (_, storage) = ssh::storage::<S>(profile).await?;
    let paths = profile.paths();
    tracking::untrack(&storage, paths, &urn, peer)?;
    Ok(())
}
