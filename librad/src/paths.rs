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

use std::{
    collections::HashMap,
    env,
    fs,
    io,
    path::{Path, PathBuf},
};

use directories::ProjectDirs;

#[derive(Clone)]
pub struct Paths {
    keys_dir: PathBuf,
    git_dir: PathBuf,
    git_includes_dir: PathBuf,
}

impl Paths {
    pub fn new() -> Result<Self, io::Error> {
        let proj = ProjectDirs::from("xyz", "radicle", "radicle").ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "Couldn't determine application directories.",
            )
        })?;

        let config_dir = proj.config_dir();
        let data_dir = proj.data_dir();

        Self {
            keys_dir: config_dir.join("keys"),
            git_dir: data_dir.join("git"),
            git_includes_dir: config_dir.join("git-includes"),
        }
        .init()
    }

    // Don't use system paths, but the supplied directory as a root.
    //
    // For testing, you know.
    pub fn from_root(root: impl AsRef<Path>) -> Result<Self, io::Error> {
        let root = root.as_ref();
        Self {
            keys_dir: root.join("keys"),
            git_dir: root.join("git"),
            git_includes_dir: root.join("git-includes"),
        }
        .init()
    }

    pub fn from_env() -> Result<Self, io::Error> {
        match env::var_os("RAD_HOME") {
            None => Self::new(),
            Some(root) => Self::from_root(root),
        }
    }

    pub fn keys_dir(&self) -> &Path {
        &self.keys_dir
    }

    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    pub fn git_includes_dir(&self) -> &Path {
        &self.git_includes_dir
    }

    pub fn all_dirs(&self) -> HashMap<&str, &Path> {
        // Nb. this pattern match is here to keep the map consistent with the
        // struct fields
        let Self {
            keys_dir,
            git_dir,
            git_includes_dir,
        } = self;

        [
            ("keys_dir", keys_dir.as_path()),
            ("git_dir", git_dir.as_path()),
            ("git_includes_dir", git_includes_dir.as_path()),
        ]
        .iter()
        .cloned()
        .collect()
    }

    fn init(self) -> Result<Self, io::Error> {
        self.all_dirs().values().try_for_each(fs::create_dir_all)?;
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Note: not testing the system paths flavour, as that would only be
    /// meaningful on a pristine system with properly set $HOME.
    #[test]
    fn test_initialises_paths() {
        let tmp = tempdir().unwrap();
        let paths = Paths::from_root(tmp.path()).unwrap();
        assert!(paths.all_dirs().values().all(|path| path.exists()))
    }

    /// Test we indeed create everything under the root dir -
    /// airquotes-chroot-airquotes.
    #[test]
    fn test_chroot() {
        let tmp = tempdir().unwrap();
        let paths = Paths::from_root(tmp.path()).unwrap();
        assert!(paths
            .all_dirs()
            .values()
            .all(|path| { path.ancestors().any(|parent| parent == tmp.path()) }))
    }
}
