// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use structopt::StructOpt;

use rad_exe::cli::args::Global;
use rad_profile::cli;

#[derive(Debug, StructOpt)]
pub struct Args {
    #[structopt(flatten)]
    pub global: Global,
    #[structopt(flatten)]
    pub profile: cli::args::Args,
}

fn main() -> anyhow::Result<()> {
    let Args { global, profile } = Args::from_args();
    rad_profile::cli::main(global.rad_ssh_auth_sock, profile)
}
