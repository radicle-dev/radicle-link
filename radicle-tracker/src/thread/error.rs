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

use crate::ops::sequence;
use std::{error, fmt};

/// Errors can occur when attempting to modify a [`crate::thread::Thread`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error<M> {
    /// We allow for errors to happen on the root element. The error is
    /// delegated to the underlying operation.
    Root(M),
    /// We may want to focus on a main thread element and modify it. Errors may
    /// occur due to the index being out of bounds, or the underlying
    /// operation fails.
    MainRoot(M),
    /// We may want to focus on a reply to a main thread element and modify it.
    /// Errors may occur due to the index being out of bounds, or the
    /// underlying operation fails.
    MainReply(sequence::Error<M>),
}

impl<M> From<M> for Error<M> {
    fn from(m: M) -> Self {
        Error::Root(m)
    }
}

impl<M> Error<M> {
    pub(crate) fn flatten_main(error: sequence::Error<Error<M>>) -> Self {
        match error {
            sequence::Error::IndexOutOfBounds(ix) => {
                Error::MainReply(sequence::Error::IndexOutOfBounds(ix))
            },
            sequence::Error::MissingModificationId(id) => {
                Error::MainReply(sequence::Error::MissingModificationId(id))
            },
            sequence::Error::Modify(err) => err,
        }
    }
}

// Writing by hand because of: https://github.com/dtolnay/thiserror/issues/79
impl<M: fmt::Display> fmt::Display for Error<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Root(m) => write!(f, "{}", m),
            Error::MainRoot(err) => write!(f, "main thread error: {}", err),
            Error::MainReply(err) => write!(f, "thread error: {}", err),
        }
    }
}

impl<M: error::Error> error::Error for Error<M> {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Error::Root(m) => m.source(),
            Error::MainRoot(err) => err.source(),
            Error::MainReply(err) => err.source(),
        }
    }
}
