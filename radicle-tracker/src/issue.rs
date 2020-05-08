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

use crate::{
    comment::{self, Comment},
    metadata::{
        clock::{Clock, RadClock},
        Label,
        Title,
    },
    ops::{
        absurd,
        fold_apply,
        replace::Replace,
        set::{self, Set},
        visibility::{self, Visibility},
        Apply,
    },
    thread::{self, Thread},
};
use std::hash::Hash;

/// Replace the title of an `Issue`.
pub type ReplaceableTitle = Replace<usize, Title>;

/// An `Issue` is a conversation created by an original [`Issue::author`]. The
/// issue is kicked off by providing a [`Title`] and an initial [`Comment`] that
/// starts the main [`Thread`].
///
/// It also contains metadata for which we would like to keep track of and
/// enhance the experience of the conversation. Labels and assignees are
/// examples of this metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue<Id, Cid, User: Eq + Hash> {
    identifier: Id,
    author: User,
    title: ReplaceableTitle,
    comments: Thread<comment::Op<User>, Comment<Cid, User>>,
    assignees: Set<User>,
    labels: Set<Label>,
    status: Visibility,
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
            status: Visibility::Visible,
        }
    }

    /// Assign a user to the issue.
    pub fn assign(&mut self, user: User) -> Op<Cid, User>
    where
        User: Clone,
    {
        Op::Assignee(self.assignees.insert(user))
    }

    /// Remove a user from the assignees of the issue.
    pub fn unassign(&mut self, user: User) -> Option<Op<Cid, User>>
    where
        User: Clone,
    {
        self.assignees.remove(user).map(Op::Assignee)
    }

    /// Assign a label to the issue.
    pub fn label(&mut self, label: Label) -> Op<Cid, User> {
        Op::Label(self.labels.insert(label))
    }

    /// Remove a label from the set of labels of the issue.
    pub fn unlabel(&mut self, label: Label) -> Option<Op<Cid, User>> {
        self.labels.remove(label).map(Op::Label)
    }

    /// A call-back style function to mutate the thread of comments of the
    /// issue.
    pub fn with_comments<F>(&mut self, f: F) -> Result<Op<Cid, User>, Error<User>>
    where
        F: FnOnce(
            &mut Thread<comment::Op<User>, Comment<Cid, User>>,
        ) -> Result<
            thread::Op<comment::Op<User>, Comment<Cid, User>>,
            thread::Error<comment::Error<User>>,
        >,
    {
        let op = f(&mut self.comments).map_err(Error::Thread)?;
        Ok(Op::Thread(op))
    }

    /// Replace the title of the issue.
    pub fn replace_title(&mut self, new_title: Title) -> Op<Cid, User> {
        self.title.replace(self.title.marker + 1, new_title);
        Op::Title(self.title.clone())
    }

    /// Close an `Issue`. This morally hides the `Issue`. Actions still be taken
    /// on the `Issue`.
    pub fn close(&mut self) -> Op<Cid, User> {
        Op::Close(self.status.hide())
    }

    /// Get a reference to the author (`User`) of this issue.
    pub fn author(&self) -> &User {
        &self.author
    }

    /// Get a reference to the [`RadClock`] of this issue.
    pub fn timestamp(&self) -> &RadClock {
        &self.timestamp
    }

    /// A function that takes an iterator of operations and applys them to the
    /// issue, mutating the state of the issue on each application.
    ///
    /// # Failures
    ///
    /// The function will fail if any of the operations fails to apply to the
    /// issue.
    pub fn fold_issue(
        &mut self,
        ops: impl Iterator<Item = Op<Cid, User>>,
    ) -> Result<(), Error<User>>
    where
        User: Hash + Eq + Ord,
    {
        fold_apply(self, ops)
    }
}

/// Operations that are the result of mutating an [`Issue`].
/// They can also be sent to other users to update their `Issue` state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op<Cid, User: Eq + Hash> {
    /// Replace the title of the issue.
    Title(ReplaceableTitle),
    /// Insert/Remove a user into/from the set of assignees.
    Assignee(set::Op<User>),
    /// Insert/Remove a label into/from the set of label.
    Label(set::Op<Label>),
    /// An operation performed on the underlying [`Thread`] of the issue, and
    /// the [`Comment`] it affects.
    Thread(thread::Op<comment::Op<User>, Comment<Cid, User>>),
    /// Close the issue.
    Close(visibility::Op),
}

/// The errors that occur when applying an operation to an `Issue`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error<User> {
    /// The assignee operation failed.
    Assignee(set::Error<User>),
    /// The label operation failed.
    Label(set::Error<Label>),
    /// The thread or comment operation failed.
    Thread(thread::Error<comment::Error<User>>),
}

impl<Id, Cid, User: Eq + Hash + Ord> Apply for Issue<Id, Cid, User> {
    type Op = Op<Cid, User>;
    type Error = Error<User>;

    fn apply(&mut self, op: Self::Op) -> Result<(), Self::Error> {
        match op {
            Op::Title(title) => self.title.apply(title).map_err(absurd),
            Op::Assignee(assignee) => self.assignees.apply(assignee).map_err(Error::Assignee),
            Op::Label(label) => self.labels.apply(label).map_err(Error::Label),
            Op::Thread(comment) => self.comments.apply(comment).map_err(Error::Thread),
            Op::Close(close) => self.status.apply(close).map_err(absurd),
        }
    }
}
