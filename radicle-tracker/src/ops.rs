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

use crate::{metadata::*, thread::*};
use std::hash::Hash;

trait Commute {
    type Input;
    type Item;
    fn op(a: Self::Input, b: Self::Item) -> Self;
}

impl Commute for i8 {
    type Input = i8;
    type Item = i8;

    fn op(a: Self::Input, b: Self::Item) -> Self {
        a + b
    }
}

pub enum MetaOp<User> {
    AddAssignee(User),
    RemoveAssignee(User),
    AddLabel(Label),
    RemoveLabel(Label),
}

impl<User> MetaOp<User> {
    pub fn effect(self, metadata: &mut Metadata<User>) -> bool
    where
        User: Hash + Eq,
    {
        match self {
            MetaOp::AddAssignee(user) => metadata.add_assignee(user),
            MetaOp::RemoveAssignee(user) => metadata.remove_assignee(&user),
            MetaOp::AddLabel(label) => metadata.add_label(label),
            MetaOp::RemoveLabel(label) => metadata.remove_label(&label),
        }
    }
}

pub enum CommentOp<User> {
    ReactTo(Reaction<User>),
    UnreactTo(Reaction<User>),
}

impl<User> CommentOp<User> {
    pub fn effect<Id>(self, comment: &mut Comment<Id, User>) -> bool
    where
        User: Eq + Hash,
    {
        match self {
            CommentOp::ReactTo(reaction) => comment.react(reaction),
            CommentOp::UnreactTo(reaction) => comment.unreact(&reaction),
        }
    }
}

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

pub struct NewThread<A>(A);

impl<A> NewThread<A> {
    pub fn effect(self) -> Thread<A> {
        Thread::new(self.0)
    }
}

pub enum ThreadOp<A, F: FnOnce(&mut A) -> ()> {
    Reply(A, ReplyTo),
    Delete,
    Edit(F),
}

impl<A, F: FnOnce(&mut A) -> ()> ThreadOp<A, F> {
    pub fn effect(self, finger: Finger, thread: &mut Thread<A>) -> Result<(), Error>
    where
        A: Clone,
    {
        thread.navigate_to(finger)?;
        match self {
            ThreadOp::Reply(a, reply_to) => {
                thread.reply(a, reply_to);
                Ok(())
            },
            ThreadOp::Delete => thread.delete(),
            ThreadOp::Edit(f) => {
                thread.edit(f);
                Ok(())
            },
        }
    }
}
