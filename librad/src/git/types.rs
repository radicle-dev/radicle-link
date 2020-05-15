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

use crate::{hash::Hash, peer::PeerId};

pub type Namespace = Hash;

#[derive(Clone, Copy)]
pub enum RefsCategory {
    Heads,
    Rad,
}

impl Display for RefsCategory {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Heads => f.write_str("heads"),
            Self::Rad => f.write_str("rad"),
        }
    }
}

#[derive(Clone)]
pub struct Reference {
    pub namespace: Namespace,
    pub remote: Option<PeerId>,
    pub category: RefsCategory,
    pub name: String, // TODO: apply validation like `uri::Path`
}

impl Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "refs/namespaces/{}/refs/", self.namespace)?;

        match &self.remote {
            None => write!(f, "{}/{}", self.category, self.name),
            Some(remote) => write!(f, "remotes/{}/{}/{}", remote, self.category, self.name),
        }
    }
}

#[derive(Clone)]
pub struct Refspec {
    pub remote: Reference,
    pub local: Reference,
    pub force: bool,
}

impl Display for Refspec {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.force {
            f.write_str("+")?;
        }
        write!(f, "{}:{}", self.remote, self.local)
    }
}
