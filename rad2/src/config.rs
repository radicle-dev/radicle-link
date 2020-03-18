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

use std::{fmt::Debug, path::PathBuf};

use anyhow::Error;
use structopt::StructOpt;

use keystore::Keystore;
use librad::paths::Paths;

/// Common options
#[derive(StructOpt)]
pub struct CommonOpts {
    /// Verbose output
    #[structopt(short, long)]
    pub verbose: bool,

    /// Override the default, platform-specific configuration and state
    /// directory root
    ///
    /// Most useful for local testing with multiple identities.
    #[structopt(long, env = "RAD_ROOT", parse(from_os_str))]
    pub paths: Option<PathBuf>,
}

/// Stateful configuration, derived from [`CommonOpts`] and passed around to
/// commands.
pub struct Config<K> {
    pub verbose: bool,
    pub paths: Paths,
    pub keystore: K,
}

impl CommonOpts {
    pub fn into_config<F, K>(self, init_keystore: F) -> Result<Config<K>, Error>
    where
        F: FnOnce(&Paths) -> K,
        K: Keystore,
        K::Error: Debug + Send + Sync,
    {
        let verbose = self.verbose;
        let paths = if let Some(root) = self.paths {
            Paths::from_root(&root)
        } else {
            Paths::new()
        }?;
        let keystore = init_keystore(&paths);

        Ok(Config {
            verbose,
            paths,
            keystore,
        })
    }
}
