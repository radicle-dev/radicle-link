// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use clap::Parser;

use super::args::{self, Args};

pub fn main() -> anyhow::Result<()> {
    let Args { global, command } = Args::parse();

    tracing_subscriber::fmt::init();

    // TODO: provide Runtime trait in lnk-clib to allow different runtimes
    let runtime = tokio::runtime::Builder::new_current_thread()
        .thread_name("lnk")
        .enable_all()
        .build()
        .unwrap();

    match command {
        args::Command::Identities(args) => {
            lnk_identities::cli::main(args, global.lnk_profile, global.lnk_ssh_auth_sock)
        },
        args::Command::Profile(args) => lnk_profile::cli::main(args, global.lnk_ssh_auth_sock),
        args::Command::Sync(args) => {
            lnk_sync::cli::main(args, global.lnk_profile, global.lnk_ssh_auth_sock, runtime)
        },
    }
}
