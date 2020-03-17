// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

extern crate radicle_keystore as keystore;

use std::{process::exit, time::SystemTime};

use structopt::StructOpt;

use keystore::{crypto::Pwhash, pinentry::Prompt, Keystore};
use librad::keys::device;

mod commands;
mod config;
mod editor;
pub mod error;

use crate::config::CommonOpts;

#[derive(StructOpt)]
#[structopt(about)]
struct Rad2 {
    #[structopt(flatten)]
    common: CommonOpts,

    #[structopt(subcommand)]
    cmd: Commands,
}

#[derive(StructOpt)]
enum Commands {
    /// Manage keys
    Keys(commands::keys::Commands),
    /// Manage projects
    Project(commands::project::Commands),
    /// Manage user profiles
    Profiles(commands::profiles::Commands),
}

type KeystoreImpl =
    keystore::FileStorage<Pwhash<Prompt<'static>>, device::PublicKey, device::Key, SystemTime>;

fn main() -> Result<(), error::Error<<KeystoreImpl as Keystore>::Error>> {
    if !librad::init() {
        eprintln!("Failed to initialise librad2");
        exit(1);
    }

    let app: Rad2 = StructOpt::from_args();
    let cfg = app.common.into_config(|paths| {
        keystore::FileStorage::new(
            &paths.keys_dir().join("device.key"),
            Pwhash::new(Prompt::new("Unlock your keystore:")),
        )
    })?;

    match app.cmd {
        Commands::Keys(cmd) => cmd.run(cfg),
        Commands::Project(cmd) => cmd.run(cfg),
        Commands::Profiles(cmd) => cmd.run(cfg).map_err(|e| e.into()),
    }
}
