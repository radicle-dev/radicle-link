use std::{fmt::Debug, time::SystemTime};

use structopt::StructOpt;

use keystore::Keystore;
use librad::keys::device;

use crate::{config::Config, error::Error};

#[derive(StructOpt)]
/// Manage keys
pub enum Commands {
    /// Create new keys
    New,
    /// Show available keys
    Show,
}

impl Commands {
    pub fn run<K>(self, cfg: Config<K>) -> Result<(), Error<K::Error>>
    where
        K: Keystore<PublicKey = device::PublicKey, SecretKey = device::Key, Metadata = SystemTime>,
        K::Error: Debug + Send + Sync,
    {
        match self {
            Self::New => {
                let key = device::Key::new();
                let mut store = cfg.keystore;
                store.put_key(key).map_err(Error::Keystore)
            },
            Self::Show => cfg
                .keystore
                .show_key()
                .map_err(Error::Keystore)
                .map(|key| println!("Device key: {:?}", key)),
        }
    }
}
