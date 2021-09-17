// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use structopt::StructOpt;

use rad_exe::cli::args::Global;
use rad_identities::cli;

#[derive(Debug, StructOpt)]
pub struct Args {
    #[structopt(flatten)]
    pub global: Global,
    #[structopt(flatten)]
    pub identities: cli::args::Args,
}

fn main() -> anyhow::Result<()> {
    let Args { global, identities } = Args::from_args();
    rad_identities::cli::main(identities, global.rad_profile, global.rad_ssh_auth_sock)
}
