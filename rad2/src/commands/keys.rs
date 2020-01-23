use failure::Fail;
use structopt::StructOpt;

use librad::keys::{
    device,
    storage,
    storage::{Pinentry, Storage},
};

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
    pub fn run<K, P>(self, cfg: Config<K, P>) -> Result<(), Error<P::Error>>
    where
        K: Storage<P>,
        P: Pinentry,
        P::Error: Fail,
    {
        match self {
            Self::New => {
                let key = device::Key::new();
                let mut store = cfg.keystore;
                store.put_device_key(&key).map_err(|e| e.into())
            },
            Self::Show => cfg
                .keystore
                .get_device_key()
                .map_err(|e| match e {
                    storage::Error::NoSuchKey => Error::EmptyKeyStore,
                    _ => e.into(),
                })
                .map(|key| println!("Device key: {}", key)),
        }
    }
}
