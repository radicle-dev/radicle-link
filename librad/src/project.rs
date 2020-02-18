use std::{fmt, fmt::Display, ops::Deref, path::PathBuf, str::FromStr};

use serde::{Deserializer, Serializer};

use crate::{git, git::GitProject, meta, paths::Paths};

#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display = "{}", 0)]
    Git(git::Error),
}

impl From<git::Error> for Error {
    fn from(err: git::Error) -> Self {
        Self::Git(err)
    }
}

/// An opaque project identifier.
///
/// Currently only supports [`git::ProjectId`], but may support other backends
/// in the future.
///
/// [`git::ProjectId`]: ../git/struct.ProjectId.html
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectId(git::ProjectId);

impl ProjectId {
    pub fn path(&self, paths: &Paths) -> PathBuf {
        paths.projects_dir().join(self.to_string())
    }
}

pub mod projectid {
    use crate::git;

    #[derive(Debug, Fail)]
    pub enum ParseError {
        #[fail(display = "{}", 0)]
        Git(git::projectid::ParseError),
    }

    impl From<git::projectid::ParseError> for ParseError {
        fn from(err: git::projectid::ParseError) -> Self {
            Self::Git(err)
        }
    }
}

impl FromStr for ProjectId {
    type Err = projectid::ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        git::ProjectId::from_str(s).map(Self).map_err(|e| e.into())
    }
}

impl Display for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<git::ProjectId> for ProjectId {
    fn from(pid: git::ProjectId) -> Self {
        Self(pid)
    }
}

// FIXME(kim): for now, serde via `Display`/`FromStr`. define a compact binary
// representation.

impl serde::Serialize for ProjectId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for ProjectId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// Stateful project handle.
///
/// Currently only supports [`GitProject`], but may support other backends
/// in the future.
///
/// [`GitProject`]: ../git/struct.GitProject.html
pub enum Project {
    Git(git::GitProject),
}

impl Project {
    /// Open a project handle.
    pub fn open(paths: &Paths, id: &ProjectId) -> Result<Project, Error> {
        GitProject::open(&id.path(paths))
            .map(Project::Git)
            .map_err(|e| e.into())
    }

    /// Get the latest project metadata for project `id`.
    pub fn show(paths: &Paths, id: &ProjectId) -> Result<meta::Project, Error> {
        GitProject::open(&id.path(paths))?
            .metadata()
            .map_err(|e| e.into())
    }

    /// List all known projects.
    ///
    /// TODO: Return more info than just `ProjectId`
    pub fn list(paths: &Paths) -> impl Iterator<Item = ProjectId> {
        paths
            .projects_dir()
            .read_dir()
            .expect("Can't read projects dir!")
            .filter_map(|dir_entry| {
                if let Ok(entry) = dir_entry {
                    match entry.file_type() {
                        Ok(ft) if ft.is_dir() => {
                            let fname = entry.file_name();
                            let name = fname.to_string_lossy();
                            if name.deref().ends_with(".git") {
                                ProjectId::from_str(&*name).ok()
                            } else {
                                None
                            }
                        },
                        _ => None,
                    }
                } else {
                    None
                }
            })
    }
}
