// Copyright © 2019-2021 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    env,
    fmt,
    io,
    path::{Path, PathBuf},
};

use crate::paths::{project_dirs, Paths};

pub mod id;
pub use id::ProfileId;

pub const LNK_HOME: &str = "LNK_HOME";
pub const LNK_PROFILE: &str = "LNK_PROFILE";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the profile {0} does not exist")]
    DoesNotExist(ProfileId),
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LnkHome {
    /// The system specific directories given by [`directories::ProjectDirs`].
    ProjectDirs,
    /// A given path, usually set through `LNK_HOME`.
    Root(PathBuf),
}

impl Default for LnkHome {
    /// If `LNK_HOME` is defined then the path supplied there is used and
    /// [`LnkHome::Root`] is constructed. Otherwise, [`LnkHome::ProjectDirs`] is
    /// constructed.
    fn default() -> Self {
        if let Ok(root) = env::var(LNK_HOME) {
            Self::Root(Path::new(&root).to_path_buf())
        } else {
            Self::ProjectDirs
        }
    }
}

impl fmt::Display for LnkHome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl LnkHome {
    fn config(&self) -> Result<PathBuf, io::Error> {
        Ok(match self {
            Self::ProjectDirs => project_dirs()?.config_dir().to_path_buf(),
            Self::Root(root) => root.clone(),
        })
    }
}

impl Profile {
    /// Create a new `Profile` by generating a new [`ProfileId`] and creating
    /// the directory structure under the [`LnkHome`] given. Note that this
    /// will not set the active profile, to do that use [`Profile::set`].
    pub fn new(home: &LnkHome) -> Result<Self, Error> {
        let id = ProfileId::new();
        Self::from_home(home, Some(id))
    }

    pub fn active(home: &LnkHome) -> Result<Option<Self>, Error> {
        let id = ProfileId::active(home)?;
        id.map(|id| Self::from_home(home, Some(id))).transpose()
    }

    /// Get the `Profile` to be found by `id` under `home`. If it does not exist
    /// then `None` is retured.
    pub fn get(home: &LnkHome, id: ProfileId) -> Result<Option<Self>, Error> {
        if !exists(home, &id)? {
            return Ok(None);
        }
        Self::from_home(home, Some(id)).map(Some)
    }

    /// Set the `"active_profile"` under `home` to the given `id`. This will
    /// error if the `id` does not exist under `home`. To ensure that the
    /// `ProfileId` exists, use [`Profile::get`].
    pub fn set(home: &LnkHome, id: ProfileId) -> Result<Self, Error> {
        if !exists(home, &id)? {
            return Err(Error::DoesNotExist(id));
        }
        id.set_active(home)?;
        Self::from_home(home, Some(id))
    }

    /// List all the `Profile`s that can be found under `home`.
    ///
    /// Note: It is expected that only [`ProfileId`]s exist under `home`.
    pub fn list(home: &LnkHome) -> Result<Vec<Self>, Error> {
        let mut profiles = Vec::new();
        let config = home.config()?;
        for entry in config.read_dir()? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let id = name.to_string_lossy().parse()?;
            let profile = Self::from_home(home, Some(id))?;
            profiles.push(profile)
        }
        Ok(profiles)
    }

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
    /// If the `LNK_PROFILE` environment variable is set, its value is used as
    /// the profile identifier. The `active_profile` file is ignored.
    ///
    /// If the `LNK_HOME` environment variable is set, its value is used instead
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
        let home = LnkHome::default();
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
    pub fn from_home(home: &LnkHome, profile_id: Option<ProfileId>) -> Result<Self, Error> {
        let id = match profile_id {
            Some(id) => id,
            None => ProfileId::load(home)?,
        };

        let paths = match home {
            LnkHome::ProjectDirs => Paths::new(&id.0)?,
            LnkHome::Root(root) => Paths::from_root(root.join(&id.0))?,
        };

        Ok(Self { id, paths })
    }

    pub fn from_root(root: &Path, profile_id: Option<ProfileId>) -> Result<Self, Error> {
        Self::from_home(&LnkHome::Root(root.to_path_buf()), profile_id)
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

fn exists(home: &LnkHome, id: &ProfileId) -> Result<bool, Error> {
    let config = home.config()?;
    let path = config.join(id.as_str());
    Ok(path.is_dir())
}
