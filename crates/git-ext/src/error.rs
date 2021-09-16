// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

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
