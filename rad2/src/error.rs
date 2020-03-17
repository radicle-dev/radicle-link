use std::{
    fmt::{Debug, Display},
    io,
};

use failure::Fail;
use git2;

use librad::{git, keys::pgp, project};

use crate::{commands::profiles, editor};

#[derive(Debug, Fail)]
pub enum Error<S>
where
    S: Debug + Display + Send + Sync + 'static,
{
    #[fail(display = "{}", 0)]
    Cli(String),

    #[fail(display = "{}", 0)]
    Keystore(S),

    #[fail(display = "{}", 0)]
    Io(io::Error),

    #[fail(display = "{}", 0)]
    Pgp(pgp::Error),

    #[fail(display = "{}", 0)]
    Git(git::Error),

    #[fail(display = "{}", 0)]
    Libgit(git2::Error),

    #[fail(display = "{}", 0)]
    Editor(editor::Error),

    #[fail(display = "{}", 0)]
    Profiles(profiles::Error),

    #[fail(display = "{}", 0)]
    Project(project::Error),
}

impl<S: Debug + Display + Send + Sync> From<io::Error> for Error<S> {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl<S: Debug + Display + Send + Sync> From<pgp::Error> for Error<S> {
    fn from(err: pgp::Error) -> Self {
        Self::Pgp(err)
    }
}

impl<S: Debug + Display + Send + Sync> From<git::Error> for Error<S> {
    fn from(err: git::Error) -> Self {
        Self::Git(err)
    }
}

impl<S: Debug + Display + Send + Sync> From<git2::Error> for Error<S> {
    fn from(err: git2::Error) -> Self {
        Self::Libgit(err)
    }
}

impl<S: Debug + Display + Send + Sync> From<profiles::Error> for Error<S> {
    fn from(err: profiles::Error) -> Self {
        Self::Profiles(err)
    }
}

impl<S: Debug + Display + Send + Sync> From<project::Error> for Error<S> {
    fn from(err: project::Error) -> Self {
        Self::Project(err)
    }
}
