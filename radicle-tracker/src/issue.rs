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

//! An [`Issue`] is a conversation that forms around code collaboration. When a
//! user creates a new issue to interact with they kick off a [`Thread`]. The
//! thread is made up of a main thread that other users may reply to. On each
//! comment on the main thread a single sub-thread can also happen.
//!
//! Issues are more than just conversations, they also carry [`Metadata`]
//! alongside them so that we can enrich the experience of our conversations,
//! allowing us to label for organisation, react for emotions, and assign to
//! users to help responsibility.

use crate::{
    metadata::{
        clock::{Clock, RadClock},
        *,
    },
    thread::Thread,
};
use std::hash::Hash;

/// An [`Issue`] that has been closed. The underlying issue cannot be mutated,
/// and can we can only access the reference of this issue..
#[derive(Debug, Clone)]
pub struct ClosedIssue<Id, Cid, User: Eq + Hash>(Issue<Id, Cid, User>);

impl<Id, Cid, User: Eq + Hash> ClosedIssue<Id, Cid, User> {
    /// Reopen the underlying [`Issue`].
    pub fn reopen(self) -> Issue<Id, Cid, User> {
        self.0
    }

    /// Get a reference to the underlying [`Issue`].
    /// This can be combined with any functionality that uses the `Issue` as a
    /// reference. For example, we could follow this call with
    /// [`Issue::thread`] and `clone` to view a thread's comments.
    pub fn issue(&self) -> &Issue<Id, Cid, User> {
        &self.0
    }
}

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
    title: Title,
    thread: Thread<Comment<Cid, User>>,
    meta: Metadata<User>,
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
            title,
            thread: Thread::new(comment),
            meta: Metadata::new(),
            timestamp,
        }
    }

    /// Close an [`Issue`] and get back a [`ClosedIssue`]. This limits the
    /// functionality on the original `Issue`.
    pub fn close(self) -> ClosedIssue<Id, Cid, User> {
        ClosedIssue(self)
    }

    /// Get a reference to the author (`User`) of this issue.
    pub fn author(&self) -> &User {
        &self.author
    }

    /// Get a reference to the [`Title`] of this issue.
    pub fn title(&self) -> &Title {
        &self.title
    }

    /// Get a reference to the [`Thread`] of this issue.
    pub fn thread(&self) -> &Thread<Comment<Cid, User>> {
        &self.thread
    }

    /// Get a mutable reference to the [`Thread`] of this issue.
    pub fn thread_mut(&mut self) -> &mut Thread<Comment<Cid, User>> {
        &mut self.thread
    }

    /// Get a reference to the [`Metadata`] of this issue.
    pub fn meta(&self) -> &Metadata<User> {
        &self.meta
    }

    /// Get a mutable reference to the [`Metadata`] of this issue.
    pub fn meta_mut(&mut self) -> &mut Metadata<User> {
        &mut self.meta
    }
}
