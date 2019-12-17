use failure::Fail;
use structopt::StructOpt;

use librad::keys::device;
use librad::keys::storage;
use librad::keys::storage::{FileStorage, Pinentry, Storage};
use librad::paths::Paths;

use crate::error::Error;

#[derive(StructOpt)]
/// Manage keys
pub enum Commands {
    /// Create new keys
    New,
    /// Show available keys
    Show,
}

pub fn run<F, P>(paths: Paths, cmd: Commands, verbose: bool, pin: F) -> Result<(), Error<P::Error>>
where
    F: FnOnce(&'static str) -> P,
    P: Pinentry,
    P::Error: Fail,
{
    match cmd {
        Commands::New => create_device_key(paths, verbose, pin),
        Commands::Show => show_keys(paths, verbose, pin).map_err(|e| match e {
            Error::Storage(storage::Error::NoSuchKey) => Error::EmptyKeyStore,
            _ => e,
        }),
    }
}

fn create_device_key<F, P>(paths: Paths, _verbose: bool, pin: F) -> Result<(), Error<P::Error>>
where
    F: FnOnce(&'static str) -> P,
    P: Pinentry,
    P::Error: Fail,
{
    let mut store = FileStorage::new(paths);
    let key = device::Key::new();
    store
        .put_device_key(&key, pin("Enter a passphrase for your device key:"))
        .map_err(|e| e.into())
}

fn show_keys<F, P>(paths: Paths, verbose: bool, pin: F) -> Result<(), Error<P::Error>>
where
    F: FnOnce(&'static str) -> P,
    P: Pinentry,
    P::Error: Fail,
{
    let store = FileStorage::new(paths);
    store
        .get_device_key(pin("Unlock your key store:"))
        .map(|key| {
            if verbose {
                println!("Device Key: {} ({:?})", key, store.key_file_path())
            } else {
                println!("Device Key: {}", key)
            }
        })
        .map_err(|e| e.into())
}
