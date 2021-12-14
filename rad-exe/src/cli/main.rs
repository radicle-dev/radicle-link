// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    io::ErrorKind,
    process::{exit, Command},
};

use structopt::StructOpt;

use super::args::{self, sanitise_globals, Args};

pub fn main() -> anyhow::Result<()> {
    let Args { global, command } = sanitise_globals(Args::from_args());
    match command {
        args::Command::Identities(args) => {
            rad_identities::cli::main(args, global.rad_profile, global.rad_ssh_auth_sock)
        },
        args::Command::Profile(args) => rad_profile::cli::main(args, global.rad_ssh_auth_sock),
        args::Command::External(external) => {
            let exe = external.first();
            match exe {
                Some(exe) => {
                    let exe = format!("rad-{}", exe);
                    let status = Command::new(exe.clone()).args(&external[1..]).status();
                    match status {
                        Ok(status) => {
                            anyhow::ensure!(status.success(), status);
                            Ok(())
                        },
                        Err(err) => {
                            if let ErrorKind::NotFound = err.kind() {
                                eprintln!("{} not found", exe);
                                exit(1)
                            } else {
                                Err(err.into())
                            }
                        },
                    }
                },
                None => {
                    eprintln!("no subcommand was provided");
                    Ok(())
                },
            }
        },
    }
}
