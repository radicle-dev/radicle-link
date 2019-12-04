extern crate librad2;

use failure::Fail;
use std::process::exit;
use structopt::StructOpt;

use librad2::keys::storage::Pinentry;
use librad2::paths::Paths;

mod commands;
mod editor;
pub mod error;
mod pinentry;

#[derive(StructOpt)]
#[structopt(about)]
struct Rad2 {
    #[structopt(short, long)]
    /// Verbose output
    verbose: bool,

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

fn main() {
    if !librad2::init() {
        eprintln!("Failed to initialise librad2");
        exit(1);
    }

    let pin = pinentry::Pinentry::new;
    let app = StructOpt::from_args();
    exit(match run(app, pin) {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("{}", e);
            1
        }
    });
}

fn run<F, P>(app: Rad2, pin: F) -> Result<(), error::Error<P::Error>>
where
    F: FnOnce(&'static str) -> P,
    P: Pinentry,
    P::Error: Fail,
{
    let paths = Paths::new()?;
    match app.cmd {
        Commands::Keys(cmd) => commands::keys::run(paths, cmd, app.verbose, pin),
        Commands::Project(cmd) => commands::project::run(paths, cmd, app.verbose, pin),
        Commands::Profiles(cmd) => {
            commands::profiles::run(paths, cmd, app.verbose).map_err(|e| e.into())
        }
    }
}
