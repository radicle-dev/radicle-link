use clap::{App, Arg, ArgMatches};
use failure::Fail;

use librad2::keys::device;
use librad2::keys::storage;
use librad2::keys::storage::{FileStorage, Pinentry, Storage};
use librad2::paths::Paths;

use crate::error::Error;

pub fn commands() -> App<'static, 'static> {
    App::new("keys")
        .about("manage keys")
        .arg(
            Arg::with_name("verbose")
                .short("v")
                .multiple(true)
                .help("Show additional information"),
        )
        .subcommand(App::new("new"))
}

pub fn dispatch<'a, F, P>(args: &ArgMatches<'a>, pin: F) -> Result<(), Error<P::Error>>
where
    F: FnOnce(&'static str) -> P,
    P: Pinentry,
    P::Error: Fail,
{
    let verbose = args.is_present("verbose");
    if args.subcommand_matches("new").is_some() {
        create_device_key(pin)
    } else {
        show_keys(verbose, pin).map_err(|e| match e {
            Error::StorageError(storage::Error::NoSuchKey) => Error::EmptyKeyStore,
            _ => e,
        })
    }
}

fn create_device_key<F, P>(pin: F) -> Result<(), Error<P::Error>>
where
    F: FnOnce(&'static str) -> P,
    P: Pinentry,
    P::Error: Fail,
{
    let mut store = FileStorage::new(Paths::new()?);
    let key = device::Key::new();
    store
        .put_device_key(&key, pin("Enter a passphrase for your device key:"))
        .map_err(|e| e.into())
}

fn show_keys<F, P>(verbose: bool, pin: F) -> Result<(), Error<P::Error>>
where
    F: FnOnce(&'static str) -> P,
    P: Pinentry,
    P::Error: Fail,
{
    let store = FileStorage::new(Paths::new()?);
    store
        .get_device_key(pin("Unlock your key store:"))
        .map(|key| {
            if verbose {
                println!(
                    "Device Key: {} (stored at: {:?})",
                    key,
                    store.key_file_path()
                )
            } else {
                println!("Device Key: {}", key)
            }
        })
        .map_err(|e| e.into())
}
