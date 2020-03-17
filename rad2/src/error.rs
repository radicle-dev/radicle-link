// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

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
