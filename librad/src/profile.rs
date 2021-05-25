// Copyright © 2019-2021 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    env,
    fs,
    io,
    path,
    path::{Path, PathBuf},
};
use uuid::Uuid;

use crate::paths::{project_dirs, Paths};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid profile ID in RAD_PROFILE environment variable: {id}")]
    InvalidProfileIdFromEnv { id: String },
    #[error("invalid profile ID loaded from {path}: {id}")]
    InvalidProfileIdFromFile { id: String, path: PathBuf },
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// A [`Profile`] provides [`Paths`] scoped by an identifier.
///
/// Profiles with different identifiers have distinct paths.
#[derive(Debug, Clone)]
pub struct Profile {
    id: String,
    paths: Paths,
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
    /// `$XDG_CONFIG_HOME/radicle-link/active_profile` and profile specific
    /// system paths are `$XDG_CONFIG_HOME/radicle-link/<profile_id>` and
    /// `XDG_DATA_HOME/radicle-link/<profile-id>`.
    pub fn load() -> Result<Self, Error> {
        let env_profile_id = env::var("RAD_PROFILE").ok();

        if let Some(ref id) = env_profile_id {
            if !is_valid_profile_id(id) {
                return Err(Error::InvalidProfileIdFromEnv { id: id.clone() });
            }
        }

        if let Ok(rad_home) = env::var("RAD_HOME") {
            Self::from_root(Path::new(&rad_home), env_profile_id)
        } else {
            let id = if let Some(id) = env_profile_id {
                id
            } else {
                load_profile_id(project_dirs()?.config_dir())?
            };

            let paths = Paths::new(&id)?;

            Ok(Self { id, paths })
        }
    }

    /// Creates a profile where `<root>/<profile_id>` is used as the base path
    /// for profile specific data.
    ///
    /// If `profile_id` is `None`, then the profile is read from
    /// `<root>/active_profile` if the file exists. Otherwise, a new profile
    /// ID is generated and written to `<root>/active_profile`.
    ///
    /// [`Paths::from_root`] for more information.
    pub fn from_root(root: &Path, profile_id: Option<String>) -> Result<Self, Error> {
        let id = if let Some(profile_id) = profile_id {
            profile_id
        } else {
            load_profile_id(root)?
        };

        let paths = Paths::from_root(root.join(&id))?;

        Ok(Self { id, paths })
    }

    /// Returns the profile identifier
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns [`Paths`] for this profile.
    pub fn paths(&self) -> &Paths {
        &self.paths
    }
}

/// Returns `true` if `id` is a valid profile ID.
///
/// A valid profile ID is not empty, does not contain path separators, is not
/// a windows path prefix like `C:`, and is not a special component like `.` or
/// `..`.
fn is_valid_profile_id(id: &str) -> bool {
    let mut components = Path::new(id).components();

    match components.next() {
        Some(path::Component::Normal(_)) => {},
        _ => return false,
    }

    if components.next().is_some() {
        return false;
    }

    true
}

/// Read the profile ID from a file or create the file and write a newly
/// generated profile ID to it.
///
/// The profile ID is the first line of the file. If the file does not exist a
/// new ID is generated and written to the file.
///
/// The profile ID is validated.
pub fn load_profile_id(config_dir: &Path) -> Result<String, Error> {
    let active_profile_path = config_dir.join("active_profile");

    let maybe_id = match fs::read_to_string(&active_profile_path) {
        Ok(content) => Some(content.lines().next().unwrap_or("").to_string()),
        Err(err) => {
            if err.kind() == io::ErrorKind::NotFound {
                None
            } else {
                return Err(Error::from(err));
            }
        },
    };

    if let Some(id) = maybe_id {
        if !is_valid_profile_id(&id) {
            return Err(Error::InvalidProfileIdFromFile {
                path: active_profile_path,
                id,
            });
        }

        Ok(id)
    } else {
        let id = Uuid::new_v4().to_hyphenated().to_string();
        fs::create_dir_all(config_dir)?;
        fs::write(active_profile_path, &id)?;
        Ok(id)
    }
}
