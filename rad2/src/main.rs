extern crate radicle_keystore as keystore;

use std::{process::exit, time::SystemTime};

use structopt::StructOpt;

use keystore::{crypto::Passphrase, pinentry::Prompt, Keystore};
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
    keystore::FileStorage<Passphrase<Prompt<'static>>, device::PublicKey, device::Key, SystemTime>;

fn main() -> Result<(), error::Error<<KeystoreImpl as Keystore>::Error>> {
    if !librad::init() {
        eprintln!("Failed to initialise librad2");
        exit(1);
    }

    let app: Rad2 = StructOpt::from_args();
    let cfg = app.common.into_config(|paths| {
        keystore::FileStorage::new(
            &paths.keys_dir().join("device.key"),
            Passphrase::new(Prompt::new("Unlock your keystore:")),
        )
    })?;

    match app.cmd {
        Commands::Keys(cmd) => cmd.run(cfg),
        Commands::Project(cmd) => cmd.run(cfg),
        Commands::Profiles(cmd) => cmd.run(cfg).map_err(|e| e.into()),
    }
}
