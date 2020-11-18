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

use super::sealed;
use crate::peer::PeerId;

pub mod namespace;
pub mod reference;
pub mod refspec;
pub mod remote;

pub use namespace::{AsNamespace, Namespace};
pub use reference::{
    AsRemote,
    Many,
    Multiple,
    One,
    Reference as GenericRef,
    RefsCategory,
    Single,
    SymbolicRef,
};
pub use refspec::{Fetchspec, Pushspec, Refspec};
pub use remote::Remote;

/// Helper to aid type inference constructing a [`Reference`] without a
/// namespace.
pub struct Flat;

impl Into<Option<Namespace<git_ext::Oid>>> for Flat {
    fn into(self) -> Option<Namespace<git_ext::Oid>> {
        None
    }
}

/// Type specialised reference for the most common use within this crate.
pub type Reference<C> = GenericRef<Namespace<git_ext::Oid>, PeerId, C>;

/// Whether we should force the overwriting of a reference or not.
#[derive(Debug, Clone, Copy)]
pub enum Force {
    /// We should overwrite.
    True,
    /// We should not overwrite.
    False,
}

impl Force {
    /// Convert the Force to its `bool` equivalent.
    fn as_bool(&self) -> bool {
        match self {
            Force::True => true,
            Force::False => false,
        }
    }
}

impl From<bool> for Force {
    fn from(b: bool) -> Self {
        if b {
            Self::True
        } else {
            Self::False
        }
    }
}
