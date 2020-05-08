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
        Reaction,
    },
    ops::{
        absurd,
        replace::Replace,
        set::{self, Set},
        Apply,
    },
};
use std::{collections::HashMap, hash::Hash};

pub type Error<User> = set::Error<Reaction<User>>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReplaceOp<User> {
    pub author: User,
    pub replace: ReplaceComment,
}

pub type ReplaceComment = Replace<usize, String>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Op<User> {
    Comment(ReplaceOp<User>),
    Reaction(set::Op<Reaction<User>>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment<Id, User: Eq + Hash> {
    identifier: Id,
    author: User,
    content: ReplaceComment,
    reactions: Set<Reaction<User>>,
    timestamp: RadClock,
}

impl<Id, User: Eq + Hash> Comment<Id, User> {
    /// Create a new `Comment`.
    pub fn new(identifier: Id, author: User, content: String) -> Self {
        let timestamp = RadClock::current_time();
        Self::new_with_timestamp(identifier, author, content, timestamp)
    }

    /// Create a new `Comment` with a supplied `timestamp`.
    pub fn new_with_timestamp(
        identifier: Id,
        author: User,
        content: String,
        timestamp: RadClock,
    ) -> Self {
        Comment {
            identifier,
            author,
            content: Replace::new(content),
            reactions: Set::new(),
            timestamp,
        }
    }

    /// Get a reference to to the identifier of this comment.
    pub fn identifier(&self) -> &Id {
        &self.identifier
    }

    /// Get a reference to to the author of this comment.
    pub fn author(&self) -> &User {
        &self.author
    }

    /// Get a reference to to the content of this comment.
    pub fn content(&self) -> &String {
        &self.content.val
    }

    /// Get a reference to the timestamp of this comment.
    pub fn timestamp(&self) -> &RadClock {
        &self.timestamp
    }

    /// Add a new reaction to the set of reactions on the comment.
    /// Returns `true` if the reaction was new.
    /// Returns `false` if the reaction already existed.
    pub fn react(&mut self, reaction: Reaction<User>) -> Op<User>
    where
        User: Clone,
    {
        Op::Reaction(self.reactions.insert(reaction))
    }

    /// Add a new reaction to the set of reactions on the comment.
    /// Returns `true` if the reaction was in the set and is now removed.
    /// Returns `false` if the reaction was not in the set to begin with.
    pub fn unreact(&mut self, reaction: Reaction<User>) -> Option<Op<User>>
    where
        User: Clone,
    {
        self.reactions.remove(reaction).map(Op::Reaction)
    }

    /// Replace the content of the `Comment` with the new content.
    ///
    /// # Failures
    ///
    /// The operation will fail if the `user` isn't the same as the `author` of
    /// the `Comment`.
    pub fn replace_content(&mut self, user: User, new_content: String) -> Option<Op<User>> {
        if self.author != user {
            return None;
        }

        self.content.replace(self.content.marker + 1, new_content);
        Some(Op::Comment(ReplaceOp {
            author: user,
            replace: self.content.clone(),
        }))
    }

    /// Get the map of reactions to this comment.
    pub fn reactions(&self) -> HashMap<String, Vec<User>>
    where
        User: Clone,
    {
        let mut reaction_map = HashMap::new();
        for reaction in self.reactions.clone().0.into_iter() {
            reaction_map
                .entry(reaction.value.clone())
                .and_modify(|users: &mut Vec<User>| users.push(reaction.user.clone()))
                .or_insert_with(|| vec![reaction.user]);
        }
        reaction_map
    }
}

impl<Id, User: Eq + Hash> Apply for Comment<Id, User> {
    type Op = Op<User>;
    type Error = set::Error<Reaction<User>>;

    fn apply(&mut self, op: Self::Op) -> Result<(), Self::Error> {
        match op {
            Op::Comment(comment) => self.content.apply(comment.replace).map_err(absurd),
            Op::Reaction(reaction) => self.reactions.apply(reaction),
        }
    }
}
