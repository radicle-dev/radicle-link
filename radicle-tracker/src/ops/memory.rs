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
    metadata::{
        clock::{Clock, RadClock},
        Label,
        Reaction,
    },
    ops::metadata::{AssigneeOp, CommentOp, LabelOp},
};
use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
};

/// The metadata that is related to an issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata<User: Eq + Hash> {
    labels: Labels,
    assignees: Assignees<User>,
}

impl<User: Eq + Hash> Metadata<User> {
    /// Initialise empty metadata.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Metadata {
            labels: Labels::new(),
            assignees: Assignees::new(),
        }
    }

    /// Get a reference to the [`Label`]s in the metadata.
    pub fn labels(&self) -> &Labels {
        &self.labels
    }

    /// Add a [`Label`] to the set of labels in the metadata.
    pub fn add_label(&mut self, label: Label) -> bool {
        self.labels.add(label)
    }

    /// Remove a [`Label`] from the set of labels in the metadata.
    pub fn remove_label(&mut self, label: Label) -> bool {
        self.labels.remove(label)
    }

    pub fn contains_label(&mut self, label: &Label) -> bool {
        self.labels.contains(label)
    }

    /// Get a reference to the [`Assignees`] in the metadata.
    pub fn assignees(&self) -> &Assignees<User> {
        &self.assignees
    }

    /// Add a `User` to the set of [`Assignees`] in the metadata.
    pub fn add_assignee(&mut self, assignee: User) -> bool {
        self.assignees.add(assignee)
    }

    /// Remove a `User` from the set of [`Assignees`] in the metadata.
    pub fn remove_assignee(&mut self, assignee: User) -> bool {
        self.assignees.remove(assignee)
    }

    pub fn contains_assignee(&self, assignee: &User) -> bool {
        self.assignees.contains(assignee)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Labels(HashSet<Label>);

impl Labels {
    pub fn new() -> Self {
        Labels(HashSet::new())
    }
}

impl LabelOp<bool> for Labels {
    fn add(&mut self, label: Label) -> bool {
        self.0.insert(label)
    }

    fn remove(&mut self, label: Label) -> bool {
        self.0.remove(&label)
    }

    fn contains(&self, label: &Label) -> bool {
        self.0.contains(label)
    }
}

/// A collection of users that represent the assigned users of the issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignees<User: Eq + Hash>(HashSet<User>);

impl<User: Eq + Hash> Assignees<User> {
    pub fn new() -> Self {
        Assignees(HashSet::new())
    }
}

impl<User: Eq + Hash> AssigneeOp<User, bool> for Assignees<User> {
    fn add(&mut self, user: User) -> bool {
        self.0.insert(user)
    }

    fn remove(&mut self, user: User) -> bool {
        self.0.remove(&user)
    }

    fn contains(&self, user: &User) -> bool {
        self.0.contains(user)
    }
}

impl<User: Eq + Hash> std::ops::Deref for Assignees<User> {
    type Target = HashSet<User>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A comment of an issue is composed of its [`Comment::author`], the
/// [`Comment::content`] of the comment, and its [`Comment::reactions`].
///
/// It has a unique identifier (of type `Id`) chosen by the implementor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment<Id, User: Eq + Hash> {
    identifier: Id,
    author: User,
    content: String,
    reactions: HashSet<Reaction<User>>,
    timestamp: RadClock,
}

impl<Cid, User: Eq + Hash> Comment<Cid, User> {
    /// Create a new `Comment`.
    pub fn new(identifier: Cid, author: User, content: String) -> Self {
        let timestamp = RadClock::current_time();
        Self::new_with_timestamp(identifier, author, content, timestamp)
    }

    /// Create a new `Comment` with a supplied `timestamp`.
    pub fn new_with_timestamp(
        identifier: Cid,
        author: User,
        content: String,
        timestamp: RadClock,
    ) -> Self {
        Comment {
            identifier,
            author,
            content,
            reactions: HashSet::new(),
            timestamp,
        }
    }

    /// Get a reference to to the author of this comment.
    pub fn author(&self) -> &User {
        &self.author
    }

    /// Get a reference to to the content of this comment.
    pub fn content(&self) -> &String {
        &self.content
    }

    /// Get the map of reactions to this comment.
    pub fn reactions(&self) -> HashMap<String, Vec<User>>
    where
        User: Clone,
    {
        let mut reaction_map = HashMap::new();
        for reaction in self.reactions.clone().into_iter() {
            reaction_map
                .entry(reaction.value.clone())
                .and_modify(|users: &mut Vec<User>| users.push(reaction.user.clone()))
                .or_insert_with(|| vec![reaction.user]);
        }
        reaction_map
    }
}

impl<Id, User: Eq + Hash> CommentOp<User, bool> for Comment<Id, User> {
    /// Add a new reaction to the set of reactions on the comment.
    /// Returns `true` if the reaction was new.
    /// Returns `false` if the reaction already existed.
    fn react(&mut self, reaction: Reaction<User>) -> bool {
        self.reactions.insert(reaction)
    }

    /// Add a new reaction to the set of reactions on the comment.
    /// Returns `true` if the reaction was in the set and is now removed.
    /// Returns `false` if the reaction was not in the set to begin with.
    fn unreact(&mut self, reaction: Reaction<User>) -> bool {
        self.reactions.remove(&reaction)
    }

    fn edit<F: FnOnce(&mut String)>(&mut self, user: User, f: F) -> bool {
        let is_author = self.author == user;
        if is_author {
            f(&mut self.content);
            is_author
        } else {
            is_author
        }
    }
}
