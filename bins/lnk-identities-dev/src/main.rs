// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use clap::Parser;

use lnk_exe::cli::args::Global;
use lnk_identities::cli;

#[derive(Debug, Parser)]
pub struct Args {
    #[clap(flatten)]
    pub global: Global,
    #[clap(flatten)]
    pub identities: cli::args::Args,
}

fn main() -> anyhow::Result<()> {
    let Args { global, identities } = Args::parse();
    lnk_identities::cli::main(identities, global.lnk_profile, global.lnk_ssh_auth_sock)
}
