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

use crate::ops::appendage;
use std::{error, fmt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error<M> {
    Root(M),
    Main(appendage::Error<M>),
    Thread(appendage::Error<M>),
}

impl<M> From<M> for Error<M> {
    fn from(m: M) -> Self {
        Error::Root(m)
    }
}

impl<M> Error<M> {
    pub(crate) fn flatten_main(error: appendage::Error<appendage::Error<M>>) -> Self {
        match error {
            appendage::Error::IndexOutOfBounds(ix) => {
                Error::Main(appendage::Error::IndexOutOfBounds(ix))
            },
            appendage::Error::IndexExists(ix) => Error::Main(appendage::Error::IndexExists(ix)),
            appendage::Error::Modify(err) => Error::Main(err),
        }
    }
}

// Writing by hand because of: https://github.com/dtolnay/thiserror/issues/79
impl<M: fmt::Display> fmt::Display for Error<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Root(m) => write!(f, "{}", m),
            Error::Main(err) => write!(f, "main thread error: {}", err),
            Error::Thread(err) => write!(f, "thread error: {}", err),
        }
    }
}

impl<M: error::Error> error::Error for Error<M> {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Error::Root(m) => m.source(),
            Error::Main(err) => err.source(),
            Error::Thread(err) => err.source(),
        }
    }
}
