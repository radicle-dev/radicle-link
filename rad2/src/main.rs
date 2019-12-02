extern crate clap;
extern crate librad2;

use clap::App;
use std::process::exit;

mod commands;
pub mod error;
mod pinentry;

fn app() -> App<'static, 'static> {
    App::new("rad2").subcommand(commands::keys::commands())
}

fn main() {
    if !librad2::init() {
        eprintln!("Failed to initialise librad2");
        exit(1);
    }

    let matches = app().get_matches();
    let res = {
        if let Some(args) = matches.subcommand_matches("keys") {
            commands::keys::dispatch(args, pinentry::Pinentry::new)
        } else {
            Ok(())
        }
    };

    exit(match res {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("{}", e);
            1
        }
    });
}
