// Copyright © 2019-2021 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    env,
    io,
    path::{Path, PathBuf},
};

use crate::paths::{project_dirs, Paths};

pub mod id;
pub use id::ProfileId;

const RAD_HOME: &str = "RAD_HOME";
const RAD_PROFILE: &str = "RAD_PROFILE";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    ProfileId(#[from] id::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// A [`Profile`] provides [`Paths`] scoped by an identifier.
///
/// Profiles with different identifiers have distinct paths.
#[derive(Debug, Clone)]
pub struct Profile {
    id: ProfileId,
    paths: Paths,
}

/// An enumeration of where the root directory for a `Profile` lives.
pub enum RadHome {
    /// The system specific directories given by [`directories::ProjectDirs`].
    ProjectDirs,
    /// A given path, usually set through `RAD_HOME`.
    Root(PathBuf),
}

impl Default for RadHome {
    fn default() -> Self {
        Self::new()
    }
}

impl RadHome {
    /// If `RAD_HOME` is defined then the path supplied there is used and
    /// [`RadHome::Root`] is constructed. Otherwise, [`RadHome::ProjectDirs`] is
    /// constructed.
    pub fn new() -> Self {
        if let Ok(root) = env::var(RAD_HOME) {
            Self::Root(Path::new(&root).to_path_buf())
        } else {
            Self::ProjectDirs
        }
    }

    fn config(&self) -> Result<PathBuf, io::Error> {
        Ok(match self {
            Self::ProjectDirs => project_dirs()?.config_dir().to_path_buf(),
            Self::Root(root) => root.clone(),
        })
    }
}

impl Profile {
    /// Creates a profile by loading the profile identifier and paths from
    /// the environment variables or well-known file.
    ///
    /// By default, the profile identifier is read from the `active_profile`
    /// file in a system specific location. The paths returned by
    /// [`Profile::paths`] are based on system paths (see [`directories::
    /// ProjectDirs`]) and include the profile identifier. If the file
    /// containing the identifier does not exist, it is created and a new
    /// identifier is generated and written to the file.
    ///
    /// If the `RAD_PROFILE` environment variable is set, its value is used as
    /// the profile identifier. The `active_profile` file is ignored.
    ///
    /// If the `RAD_HOME` environment variable is set, its value is used instead
    /// of system specific project directories. See also
    /// [`Profile::from_root`].
    ///
    /// The profile identifier must not empty, must not contain path separators,
    /// must not be a windows path prefix like `C:`, and must not be a
    /// special component like `.` or `..`. Otherwise an error is returned
    ///
    /// On Linux, the path to the active profile is
    /// `$ProjectDirs_CONFIG_HOME/radicle-link/active_profile` and profile
    /// specific system paths are
    /// `$ProjectDirs_CONFIG_HOME/radicle-link/<profile_id>` and
    /// `ProjectDirs_DATA_HOME/radicle-link/<profile-id>`.
    pub fn load() -> Result<Self, Error> {
        let env_profile_id = ProfileId::from_env()?;
        let home = RadHome::new();
        Self::from_home(&home, env_profile_id)
    }

    /// Creates a profile where `<root>/<profile_id>` is used as the base path
    /// for profile specific data.
    ///
    /// If `profile_id` is `None`, then the profile is read from
    /// `<root>/active_profile` if the file exists. Otherwise, a new profile
    /// ID is generated and written to `<root>/active_profile`.
    ///
    /// [`Paths::from_root`] for more information.
    pub fn from_home(home: &RadHome, profile_id: Option<ProfileId>) -> Result<Self, Error> {
        let id = match profile_id {
            Some(id) => id,
            None => ProfileId::load(home)?,
        };

        let paths = match home {
            RadHome::ProjectDirs => Paths::new(&id.0)?,
            RadHome::Root(root) => Paths::from_root(root.join(&id.0))?,
        };

        Ok(Self { id, paths })
    }

    pub fn from_root(root: &Path, profile_id: Option<ProfileId>) -> Result<Self, Error> {
        Self::from_home(&RadHome::Root(root.to_path_buf()), profile_id)
    }

    /// Returns the profile identifier
    pub fn id(&self) -> &ProfileId {
        &self.id
    }

    /// Returns [`Paths`] for this profile.
    pub fn paths(&self) -> &Paths {
        &self.paths
    }
}
