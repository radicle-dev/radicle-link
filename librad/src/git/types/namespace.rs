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

use std::fmt::{self, Display};

use git_ext as ext;
use multihash::Multihash;

use crate::{hash::Hash, identities::git::Urn};

pub trait AsNamespace {
    fn as_namespace(&self) -> String;
}

pub type Legacy = Hash;

impl AsNamespace for Legacy {
    fn as_namespace(&self) -> String {
        self.to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Namespace(ext::Oid);

impl AsNamespace for Namespace {
    fn as_namespace(&self) -> String {
        self.to_string()
    }
}

impl From<ext::Oid> for Namespace {
    fn from(oid: ext::Oid) -> Self {
        Self(oid)
    }
}

impl From<git2::Oid> for Namespace {
    fn from(oid: git2::Oid) -> Self {
        Self::from(ext::Oid::from(oid))
    }
}

impl From<Urn> for Namespace {
    fn from(urn: Urn) -> Self {
        Self::from(urn.id)
    }
}

impl From<&Urn> for Namespace {
    fn from(urn: &Urn) -> Self {
        Self::from(urn.id)
    }
}

impl Display for Namespace {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&multibase::encode(
            multibase::Base::Base32Z,
            Multihash::from(&self.0),
        ))
    }
}
