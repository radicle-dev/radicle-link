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

use crate::{issue::*, metadata::*, thread::*};
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

pub trait Mut<A> {
    fn mutate(self, a: &mut A);
}

impl<Id, User: Eq + Hash> Mut<Comment<Id, User>> for CommentOp<User> {
    fn mutate(self, comment: &mut Comment<Id, User>) {
        self.effect(comment);
    }
}

pub enum ThreadOp<A, F: Mut<A>> {
    Reply(Finger, A, ReplyTo),
    Delete(Finger),
    Edit(Finger, F),
}

impl<A, F: Mut<A>> ThreadOp<A, F> {
    pub fn effect(self, thread: &mut Thread<A>) -> Result<(), Error> {
        match self {
            ThreadOp::Reply(finger, a, reply_to) => {
                thread.navigate_to(finger)?;
                thread.reply(a, reply_to);
                Ok(())
            },
            ThreadOp::Delete(finger) => {
                thread.navigate_to(finger)?;
                thread.delete()
            },
            ThreadOp::Edit(finger, f) => {
                thread.navigate_to(finger)?;
                thread.edit(|a| f.mutate(a));
                Ok(())
            },
        }
    }
}

pub struct NewIssue<Id, Cid, User> {
    identifier: Id,
    comment_id: Cid,
    author: User,
    title: Title,
    content: String,
}

impl<Id, Cid, User: Eq + Hash> NewIssue<Id, Cid, User> {
    pub fn effect(self) -> Issue<Id, Cid, User>
    where
        User: Clone,
    {
        Issue::new(
            self.identifier,
            self.comment_id,
            self.author,
            self.title,
            self.content,
        )
    }
}

pub enum IssueOp<Cid, User: Eq + Hash> {
    Thread(ThreadOp<Comment<Cid, User>, CommentOp<User>>),
    Meta(MetaOp<User>),
}

impl<Cid, User: Eq + Hash> IssueOp<Cid, User> {
    pub fn effect<Id>(self, issue: &mut Issue<Id, Cid, User>) -> Result<(), Error> {
        match self {
            IssueOp::Thread(op) => op.effect(issue.thread_mut()),
            IssueOp::Meta(op) => {
                op.effect(issue.meta_mut());
                Ok(())
            },
        }
    }
}
