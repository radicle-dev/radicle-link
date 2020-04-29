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

use crate::{
    metadata::{
        clock::{Clock, RadClock},
        Label,
        Title,
    },
    ops::{
        absurd,
        comment::{self, Comment},
        replace::Replace,
        set::{self, Set},
        thread::{self, Thread},
        Apply,
    },
};
use std::{convert::Infallible, hash::Hash};

pub type ReplaceableTitle = Replace<usize, Title>;

/// An `Issue` is a conversation created by an original [`Issue::author`]. The
/// issue is kicked off by providing a [`Title`] and an initial [`Comment`] that
/// starts the main [`Thread`].
///
/// It also contains [`Metadata`] for which we would like to keep track of and
/// enhance the experience of the conversation.
#[derive(Debug, Clone)]
pub struct Issue<Id, Cid, User: Eq + Hash> {
    identifier: Id,
    author: User,
    pub title: ReplaceableTitle,
    pub comments: Thread<comment::Op<User>, Comment<Cid, User>>,
    pub assignees: Set<User>,
    pub labels: Set<Label>,
    timestamp: RadClock,
}

impl<Id, Cid, User: Eq + Hash> Issue<Id, Cid, User> {
    /// Create a new `Issue`.
    pub fn new(identifier: Id, comment_id: Cid, author: User, title: Title, content: String) -> Self
    where
        User: Clone + Eq,
    {
        let timestamp = RadClock::current_time();
        Self::new_with_timestamp(identifier, comment_id, author, title, content, timestamp)
    }

    /// Create a new `Issue` with a supplied `timestamp`.
    pub fn new_with_timestamp(
        identifier: Id,
        comment_id: Cid,
        author: User,
        title: Title,
        content: String,
        timestamp: RadClock,
    ) -> Self
    where
        User: Clone + Eq,
    {
        let comment = Comment::new_with_timestamp(comment_id, author.clone(), content, timestamp);

        Issue {
            identifier,
            author,
            title: Replace::new(title),
            comments: Thread::new(comment),
            assignees: Set::new(),
            labels: Set::new(),
            timestamp,
        }
    }

    /*
    /// Close an [`Issue`] and get back a [`ClosedIssue`]. This limits the
    /// functionality on the original `Issue`.
    pub fn close(self) -> ClosedIssue<Id, Cid, User> {
        ClosedIssue(self)
    }
    */

    /// Get a reference to the author (`User`) of this issue.
    pub fn author(&self) -> &User {
        &self.author
    }

    /// Get a reference to the [`RadClock`] of this issue.
    pub fn timestamp(&self) -> &RadClock {
        &self.timestamp
    }
}

pub enum Op<Cid, User: Eq + Hash> {
    Title(ReplaceableTitle),
    Assignee(set::Op<User>),
    Label(set::Op<Label>),
    Thread(thread::Op<comment::Op<User>, Comment<Cid, User>>),
}

impl<Id, Cid, User: Eq + Hash + Ord> Apply for Issue<Id, Cid, User> {
    type Op = Op<Cid, User>;
    type Error = thread::Error<Infallible>;

    fn apply(&mut self, op: Self::Op) -> Result<(), Self::Error> {
        match op {
            Op::Title(title) => self.title.apply(title).map_err(absurd),
            Op::Assignee(assignee) => self.assignees.apply(assignee).map_err(absurd),
            Op::Label(label) => self.labels.apply(label).map_err(absurd),
            Op::Thread(comment) => self.comments.apply(comment),
        }
    }
}
