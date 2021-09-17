// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::profile::Profile;
use rad_clib::{keys::ssh::SshAuthSock, storage::ssh};

use crate::{cli::args::tracking::*, tracking};

pub fn eval_track(
    profile: &Profile,
    sock: SshAuthSock,
    Track { urn, peer }: Track,
) -> anyhow::Result<()> {
    let (_, storage) = ssh::storage(profile, sock)?;
    let paths = profile.paths();
    tracking::track(&storage, paths, &urn, peer)?;
    Ok(())
}

pub fn eval_untrack(
    profile: &Profile,
    sock: SshAuthSock,
    Untrack { urn, peer }: Untrack,
) -> anyhow::Result<()> {
    let (_, storage) = ssh::storage(profile, sock)?;
    let paths = profile.paths();
    tracking::untrack(&storage, paths, &urn, peer)?;
    Ok(())
}
