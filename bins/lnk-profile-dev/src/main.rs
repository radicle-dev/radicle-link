// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use structopt::StructOpt;

use lnk_exe::cli::args::Global;
use lnk_profile::cli;

#[derive(Debug, StructOpt)]
pub struct Args {
    #[structopt(flatten)]
    pub global: Global,
    #[structopt(flatten)]
    pub profile: cli::args::Args,
}

fn main() -> anyhow::Result<()> {
    let Args { global, profile } = Args::from_args();
    lnk_profile::cli::main(profile, global.lnk_ssh_auth_sock)
}
