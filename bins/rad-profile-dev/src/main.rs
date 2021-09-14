// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use structopt::StructOpt as _;

use rad_profile::cli::args::Args;

fn main() -> anyhow::Result<()> {
    let args = Args::from_args();
    rad_profile::cli::main(args)
}
