// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    fs,
    io,
    path::{Path, PathBuf},
};

use directories::ProjectDirs;

/// A set of paths to store application data.
///
/// The paths are either based on system specific directories when created
/// with [`crate::profile::Profile::paths`] or all contained in a given
/// directory when created with [`Paths::from_root`].
#[derive(Clone, Debug)]
pub struct Paths {
    keys_dir: PathBuf,
    git_dir: PathBuf,
    git_includes_dir: PathBuf,
}

impl Paths {
    /// Returns based on [`ProjectDirs`] and scoped to `profile_id`.
    ///
    /// On Linux, all paths start with
    /// `$XDG_{CONFIG|DATA}_HOME/radicle-link/<profile_id>`.
    pub(crate) fn new(profile_id: &str) -> Result<Self, io::Error> {
        let proj = project_dirs()?;
        let config_dir = proj.config_dir().join(profile_id);
        let data_dir = proj.data_dir().join(profile_id);

        Self {
            keys_dir: config_dir.join("keys"),
            git_dir: data_dir.join("git"),
            git_includes_dir: config_dir.join("git-includes"),
        }
        .init()
    }

    /// All paths are contained in the given directory.
    pub fn from_root(root: impl AsRef<Path>) -> Result<Self, io::Error> {
        let root = root.as_ref();
        Self {
            keys_dir: root.join("keys"),
            git_dir: root.join("git"),
            git_includes_dir: root.join("git-includes"),
        }
        .init()
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

    fn all_dirs(&self) -> impl Iterator<Item = &Path> {
        // Nb. this pattern match is here to keep the map consistent with the
        // struct fields
        let Self {
            keys_dir,
            git_dir,
            git_includes_dir,
        } = self;

        vec![
            keys_dir.as_path(),
            git_dir.as_path(),
            git_includes_dir.as_path(),
        ]
        .into_iter()
    }

    fn init(self) -> Result<Self, io::Error> {
        self.all_dirs().try_for_each(fs::create_dir_all)?;
        Ok(self)
    }
}

/// Returns [`ProjectDirs`] for this specific project (`radicle`).
///
/// Returns [`io::Error`] if the project directories could not be determined,
/// most likely due to the `$HOME` environment variable missing
pub(crate) fn project_dirs() -> Result<ProjectDirs, io::Error> {
    ProjectDirs::from("xyz", "radicle", "radicle-link").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Couldn't determine application directories.",
        )
    })
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
        assert!(paths.all_dirs().all(|path| path.exists()))
    }

    /// Test we indeed create everything under the root dir -
    /// airquotes-chroot-airquotes.
    #[test]
    fn test_chroot() {
        let tmp = tempdir().unwrap();
        let paths = Paths::from_root(tmp.path()).unwrap();
        assert!(paths
            .all_dirs()
            .all(|path| { path.ancestors().any(|parent| parent == tmp.path()) }))
    }
}
