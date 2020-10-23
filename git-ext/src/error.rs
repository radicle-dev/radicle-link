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

use std::{fmt::Display, io};

pub fn is_not_found_err(e: &git2::Error) -> bool {
    e.code() == git2::ErrorCode::NotFound
}

pub fn is_exists_err(e: &git2::Error) -> bool {
    e.code() == git2::ErrorCode::Exists
}

pub fn into_git_err<E: Display>(e: E) -> git2::Error {
    git2::Error::from_str(&e.to_string())
}

pub fn into_io_err(e: git2::Error) -> io::Error {
    io::Error::new(io::ErrorKind::Other, e)
}
