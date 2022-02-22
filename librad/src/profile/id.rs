// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    env,
    fmt,
    fs,
    io,
    path,
    path::{Path, PathBuf},
    str::FromStr,
};
use thiserror::Error;
use uuid::Uuid;

use super::{LnkHome, LNK_PROFILE};

const ACTIVE: &str = "active_profile";

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid profile ID while parsing: {id}")]
    FromStr { id: String },
    #[error("invalid profile ID in LNK_PROFILE environment variable: {id}")]
    FromEnv { id: String },
    #[error("invalid profile ID loaded from {path}: {id}")]
    FromFile { id: String, path: PathBuf },
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// An identifier that provides separation between multiple [`super::Paths`].
/// For example, two separate profiles can be created under
/// `/home/.local/share/radicle-link/0000` and
/// `/home/.local/share/radicle-link/0001`.
///
/// A valid profile ID is not empty, does not contain path separators, is
/// not a windows path prefix like `C:`, and is not a special component
/// like `.` or `..`.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProfileId(pub(super) String);

impl AsRef<Path> for ProfileId {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

impl FromStr for ProfileId {
    type Err = Error;

    fn from_str(id: &str) -> Result<Self, Self::Err> {
        ProfileId::valid(Validate::Str { id: id.to_string() })
    }
}

impl fmt::Display for ProfileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.write_str(&self.0)
    }
}

impl Default for ProfileId {
    fn default() -> Self {
        Self::new()
    }
}

enum Validate {
    File { id: String, path: PathBuf },
    Env { id: String },
    Str { id: String },
}

impl Validate {
    fn id_ref(&self) -> &str {
        match self {
            Self::File { id, .. } => id,
            Self::Env { id } => id,
            Self::Str { id } => id,
        }
    }

    fn id(self) -> String {
        match self {
            Self::File { id, .. } => id,
            Self::Env { id } => id,
            Self::Str { id } => id,
        }
    }
}

impl From<Validate> for Error {
    fn from(v: Validate) -> Error {
        match v {
            Validate::File { id, path } => Self::FromFile { id, path },
            Validate::Env { id } => Self::FromEnv { id },
            Validate::Str { id } => Self::FromStr { id },
        }
    }
}

impl ProfileId {
    /// Generate a new, hyphenated UUID-v4 identifier.
    pub fn new() -> Self {
        ProfileId(Uuid::new_v4().to_hyphenated().to_string())
    }

    /// Read a `ProfileId` from the `PROFILE_ID` environment variable. The value
    /// must pass validation, as documented in [`ProfileId`].
    pub fn from_env() -> Result<Option<Self>, Error> {
        env::var(LNK_PROFILE)
            .ok()
            .map(|id| Self::valid(Validate::Env { id }))
            .transpose()
    }

    /// Read the `ProfileId` from a file or create the file and write a newly
    /// generated `ProfileId` to it.
    pub fn load(home: &LnkHome) -> Result<Self, Error> {
        match Self::active(home)? {
            Some(id) => Ok(id),
            None => {
                let id = Self::new();
                let config = home.config()?;
                let path = config.join(ACTIVE);
                fs::create_dir_all(config)?;
                fs::write(path, &id.0)?;
                Ok(id)
            },
        }
    }

    /// Read the `ProfileId` from the `active_profile` file. If the file is not
    /// found then `None` is returned.
    ///
    /// The `ProfileId` is the first line of the file. If the file does not
    /// exist a new ID is generated and written to the file.
    pub fn active(home: &LnkHome) -> Result<Option<Self>, Error> {
        let active_path = home.config()?.join(ACTIVE);

        match fs::read_to_string(&active_path) {
            Ok(content) => {
                let id = content.lines().next().unwrap_or("").to_string();
                Self::valid(Validate::File {
                    id,
                    path: active_path,
                })
                .map(Some)
            },
            Err(err) => {
                if err.kind() == io::ErrorKind::NotFound {
                    Ok(None)
                } else {
                    Err(Error::from(err))
                }
            },
        }
    }

    pub fn set_active(&self, home: &LnkHome) -> Result<(), Error> {
        let path = home.config()?.join(ACTIVE);
        fs::write(path, &self.0)?;
        Ok(())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn valid(v: Validate) -> Result<Self, Error> {
        let id = v.id_ref();
        if Self::is_valid(id) {
            Ok(Self(v.id()))
        } else {
            Err(v.into())
        }
    }

    /// Returns `true` if `id` is a valid profile ID.
    ///
    /// A valid profile ID is not empty, does not contain path separators, is
    /// not a windows path prefix like `C:`, and is not a special component
    /// like `.` or `..`.
    fn is_valid(id: &str) -> bool {
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
}
