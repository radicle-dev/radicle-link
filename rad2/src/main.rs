extern crate librad;

use failure::Fail;
use std::process::exit;
use structopt::StructOpt;

use librad::keys::storage::{FileStorage, Pinentry, Storage};

mod commands;
mod config;
mod editor;
pub mod error;
mod pinentry;

use crate::config::{CommonOpts, Config};

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
    /// Run the p2p daemon
    Daemon(commands::daemon::Options),
}

impl Commands {
    fn run<K, P>(self, cfg: Config<K, P>) -> Result<(), error::Error<P::Error>>
    where
        K: Storage<P>,
        P: Pinentry,
        P::Error: Fail,
    {
        match self {
            Self::Keys(cmd) => cmd.run(cfg),
            Self::Project(cmd) => cmd.run(cfg),
            Self::Profiles(cmd) => cmd.run(cfg).map_err(|e| e.into()),
        }
    }
}

fn main() -> Result<(), error::Error<self::pinentry::Error>> {
    env_logger::init();
    if !librad::init() {
        eprintln!("Failed to initialise librad2");
        exit(1);
    }

    let app: Rad2 = StructOpt::from_args();
    let cfg = app.common.into_config(|paths| {
        FileStorage::new(
            paths,
            self::pinentry::Pinentry::new("Unlock your keystore:"),
        )
    })?;

    app.cmd.run(cfg)
}
