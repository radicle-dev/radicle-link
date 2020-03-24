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

use crate::metadata::{Comment, Label, Reaction};
use std::hash::Hash;

pub struct Add<I>(I);
pub struct Remove<I>(I);

pub struct AddAssignee<User>(User);
pub struct RemoveAssignee<User>(User);

pub struct AddLabel(Label);
pub struct RemoveLabel(Label);

pub struct NewComment<Id, User> {
    identity: Id,
    author: User,
    content: String,
}

impl<Id, User> NewComment<Id, User> {
    pub fn effect(self) -> Comment<Id, User>
    where
        User: Eq + Hash,
    {
        Comment::new(self.identity, self.author, self.content)
    }
}

pub struct ReactToComment<User> {
    pub reaction: Reaction<User>,
}
