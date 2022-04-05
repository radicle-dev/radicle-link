// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    fs,
    io,
    path::{Path, PathBuf},
};

use crypto::PeerId;

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
    cob_cache_dir: PathBuf,
    socket_dir: PathBuf,
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
        let cache_dir = proj.cache_dir().join(profile_id);

        Self {
            keys_dir: config_dir.join("keys"),
            git_dir: data_dir.join("git"),
            git_includes_dir: config_dir.join("git-includes"),
            cob_cache_dir: cache_dir.join("cob-cache"),
            socket_dir: socket_dir()?,
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
            cob_cache_dir: root.join("cob-cache"),
            socket_dir: socket_dir()?,
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

    pub fn cob_cache_dir(&self) -> &Path {
        &self.cob_cache_dir
    }

    pub fn all_dirs(&self) -> impl Iterator<Item = &Path> {
        // Nb. this pattern match is here to keep the map consistent with the
        // struct fields
        let Self {
            keys_dir,
            git_dir,
            git_includes_dir,
            cob_cache_dir,
            socket_dir: _,
        } = self;

        vec![
            keys_dir.as_path(),
            git_dir.as_path(),
            git_includes_dir.as_path(),
            cob_cache_dir.as_path(),
        ]
        .into_iter()
    }

    fn init(self) -> Result<Self, io::Error> {
        self.all_dirs().try_for_each(fs::create_dir_all)?;
        Ok(self)
    }

    pub fn rpc_socket(&self, peer_id: &PeerId) -> PathBuf {
        self.socket_dir
            .join(format!("link-peer-{}-rpc.socket", peer_id))
    }

    pub fn events_socket(&self, peer_id: &PeerId) -> PathBuf {
        self.socket_dir
            .join(format!("link-peer-{}-events.socket", peer_id))
    }
}

/// Returns the path to the seed configuration file.
///
/// # Error
///
/// Returns [`io::Error`] if the configuration directory could not be
/// determined, most likely due to the `$HOME` environment variable missing.
pub fn seeds() -> Result<PathBuf, io::Error> {
    Ok(project_dirs()?.config_dir().join("seeds"))
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

/// The location this returns is platform dependent:
///
/// - On linux: $XDG_RUNTIME_DIR if set, otherwise /var/run
/// - On Macos: /tmp
/// - On windows: std::env::temp_dir
fn socket_dir() -> Result<std::path::PathBuf, io::Error> {
    socket_dir_imp()
}

#[cfg(all(target_os = "macos", target_family = "unix"))]
fn socket_dir_imp() -> Result<std::path::PathBuf, io::Error> {
    Ok("/tmp".into())
}

#[cfg(all(target_family = "unix", not(target_os = "macos")))]
fn socket_dir_imp() -> Result<std::path::PathBuf, io::Error> {
    use directories::BaseDirs;

    let base = BaseDirs::new().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Couldn't determine home directory.",
        )
    })?;
    Ok(base
        .runtime_dir()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| "/tmp".into()))
}

#[cfg(target_family = "windows")]
fn socket_dir_imp() -> Result<std::path::PathBuf, io::Error> {
    Ok(std::env::temp_dir())
}
