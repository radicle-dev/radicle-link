use std::fmt;
use std::fmt::Display;
use std::ops::Deref;
use std::path::PathBuf;
use std::str::FromStr;

use crate::git;
use crate::git::GitProject;
use crate::meta;
use crate::paths::Paths;

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

#[derive(Clone, Debug, PartialEq)]
pub enum ProjectId {
    Git(git::ProjectId),
}

impl ProjectId {
    pub fn into_path(self, paths: &Paths) -> PathBuf {
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
        git::ProjectId::from_str(s)
            .map(Self::Git)
            .map_err(|e| e.into())
    }
}

impl Display for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ProjectId::Git(pid) => pid.fmt(f),
        }
    }
}

impl From<git::ProjectId> for ProjectId {
    fn from(pid: git::ProjectId) -> Self {
        Self::Git(pid)
    }
}

/// Get the latest project metadata for project `id`.
pub fn show_project(paths: &Paths, id: &ProjectId) -> Result<meta::Project, Error> {
    match id {
        ProjectId::Git(_) => {
            let proj = GitProject::open(&id.clone().into_path(paths))?.metadata()?;
            Ok(proj)
        }
    }
}

/// List all known projects.
///
/// TODO: Return more info than just `ProjectId`
pub fn list_projects(paths: &Paths) -> impl Iterator<Item = ProjectId> {
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
                    }
                    _ => None,
                }
            } else {
                None
            }
        })
}
