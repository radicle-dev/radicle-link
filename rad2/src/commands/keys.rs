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

use std::{fmt::Debug, time::SystemTime};

use anyhow::Error;
use structopt::StructOpt;

use keystore::Keystore;
use librad::keys::device;

use crate::config::Config;

#[derive(StructOpt)]
/// Manage keys
pub enum Commands {
    /// Create new keys
    New,
    /// Show available keys
    Show,
}

impl Commands {
    pub fn run<K>(self, cfg: Config<K>) -> Result<(), Error>
    where
        K: Keystore<PublicKey = device::PublicKey, SecretKey = device::Key, Metadata = SystemTime>,
        K::Error: Debug + Send + Sync + 'static,
    {
        match self {
            Self::New => {
                let key = device::Key::new();
                let mut store = cfg.keystore;
                store.put_key(key).map_err(|e| e.into())
            },
            Self::Show => cfg
                .keystore
                .show_key()
                .map_err(|e| e.into())
                .map(|key| println!("Device key: {:?}", key)),
        }
    }
}
