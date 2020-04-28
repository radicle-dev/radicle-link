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

#![allow(missing_docs)]

pub mod comment;
pub mod id;
pub mod replace;
pub mod sequence;
pub mod set;
pub mod thread;
pub mod visibility;

use crate::metadata::Label;

pub trait Apply {
    type Op;
    type Error;

    fn apply(&mut self, op: Self::Op) -> Result<(), Self::Error>;
}

pub type AssigneeOp<User> = set::Op<User>;
pub type LabelOp = set::Op<Label>;
