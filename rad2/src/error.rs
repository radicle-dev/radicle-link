use std::fmt::Debug;
use std::io;

use failure::Fail;
use git2;

use librad::git;
use librad::keys::pgp;
use librad::keys::storage;
use librad::project;

use crate::commands::profiles;
use crate::editor;

#[derive(Debug, Fail)]
pub enum Error<S: Fail> {
    #[fail(display = "{}", 0)]
    Cli(String),

    #[fail(display = "Empty key store! Create a key using `rad2 keys new`.")]
    EmptyKeyStore,

    #[fail(display = "Error: {}", 0)]
    Storage(storage::Error<S>),

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

impl<S> From<storage::Error<S>> for Error<S>
where
    S: Fail,
{
    fn from(err: storage::Error<S>) -> Self {
        Self::Storage(err)
    }
}

impl<S> From<io::Error> for Error<S>
where
    S: Fail,
{
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl<S> From<pgp::Error> for Error<S>
where
    S: Fail,
{
    fn from(err: pgp::Error) -> Self {
        Self::Pgp(err)
    }
}

impl<S> From<git::Error> for Error<S>
where
    S: Fail,
{
    fn from(err: git::Error) -> Self {
        Self::Git(err)
    }
}

impl<S> From<git2::Error> for Error<S>
where
    S: Fail,
{
    fn from(err: git2::Error) -> Self {
        Self::Libgit(err)
    }
}

impl<S> From<profiles::Error> for Error<S>
where
    S: Fail,
{
    fn from(err: profiles::Error) -> Self {
        Self::Profiles(err)
    }
}

impl<S> From<project::Error> for Error<S>
where
    S: Fail,
{
    fn from(err: project::Error) -> Self {
        Self::Project(err)
    }
}
