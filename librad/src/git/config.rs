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

//! Custom configurations

use std::{
    fs,
    io,
    ops::Deref,
    path::{Path, PathBuf},
};

use thiserror::Error;

use crate::{git::types::Namespace, peer::PeerId};

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Git(#[from] git2::Error),

    #[error(transparent)]
    Io(#[from] io::Error),
}

pub struct WorkingCopy {
    config: git2::Config,
    path: PathBuf,
}

impl Deref for WorkingCopy {
    type Target = git2::Config;

    fn deref(&self) -> &Self::Target {
        &self.config
    }
}

impl AsRef<git2::Config> for WorkingCopy {
    fn as_ref(&self) -> &git2::Config {
        self
    }
}

impl WorkingCopy {
    /// Create a new working copy configuration backed by a file.
    ///
    /// If the configuration already exists, it will be overwritten.
    pub fn new<R>(git_dir: &Path, ns: &Namespace, remotes: R) -> Result<Self, Error>
    where
        R: IntoIterator<Item = (String, PeerId)>,
    {
        let config_dir = git_dir.join("rad/working");
        fs::create_dir_all(&config_dir)?;

        let config_file = config_dir.join(ns.to_string()).with_extension("inc");
        //fs::File::create(&config_file)?;

        let mut config = git2::Config::open(&config_file)?;
        config.set_str("remote.rad.url", &git_dir.display().to_string())?;

        for x in &["heads", "tags"] {
            config.set_multivar(
                "remote.rad.push",
                "$^",
                &format!("refs/{}/*:refs/namespaces/{}/refs/{}/*", x, ns, x),
            )?;
            config.set_multivar(
                "remote.rad.fetch",
                "$^",
                &format!("+refs/namespaces/{}/refs/{}/*:refs/{}/*", ns, x, x),
            )?;
        }

        for (name, peer_id) in remotes.into_iter() {
            config.set_multivar(
                "remote.rad.fetch",
                "$^",
                &format!(
                    "+refs/namespaces/{}/refs/remotes/{}/heads/*:refs/remotes/{}@{}/heads/*",
                    ns, peer_id, name, peer_id
                ),
            )?;
        }

        Ok(Self {
            config,
            path: config_file,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
