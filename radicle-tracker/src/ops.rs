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

pub mod appendage;
pub mod id;
pub mod replace;
pub mod set;
pub mod thread;
pub mod visibility;

use replace::Replace;
use set::SetOp;

pub trait Apply {
    type Op;
    type Error;

    fn apply(&mut self, op: Self::Op) -> Result<(), Self::Error>;
}

pub struct Label {} // TODO: use real label
pub struct Assignee {} // TODO: use real assignee
pub struct Reaction {} // TODO: use real reaction

pub struct ACL<User> {
    _user: User,
} // TODO: fill in

pub type LabelOp = SetOp<Label>;
pub type AssigneeOp = SetOp<Assignee>;
pub type ReactionOp = SetOp<Reaction>;

pub struct ReplaceOp<User> {
    pub author: User,
    pub replace: Replace<usize, String>,
}

pub struct NewComment<User> {
    pub acl: ACL<User>,
    pub body: String,
    pub author: User,
}
