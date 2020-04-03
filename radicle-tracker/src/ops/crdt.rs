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

use std::collections::HashMap;

use crdts::{
    self,
    lwwreg::LWWReg,
    orswot::{self, Orswot},
    vclock::Actor,
};

use crate::{
    metadata::{
        clock::{Clock, RadClock},
        Label,
        Reaction,
    },
    ops::metadata::{AssigneeOp, CommentOp, LabelOp},
};

pub struct Labels<User: Actor> {
    actor: User,
    labels: Orswot<Label, User>,
}

impl<User: Actor> Labels<User> {
    pub fn new(actor: User) -> Self {
        Labels {
            actor,
            labels: Orswot::new(),
        }
    }

    pub fn from_labels(actor: User, labels: Orswot<Label, User>) -> Self {
        Labels { actor, labels }
    }
}

impl<User: Actor> LabelOp<orswot::Op<Label, User>> for Labels<User> {
    fn add(&mut self, label: Label) -> orswot::Op<Label, User> {
        let read = self.labels.read();
        let add_ctx = read.derive_add_ctx(self.actor.clone());
        self.labels.add(label, add_ctx)
    }

    fn remove(&mut self, label: Label) -> orswot::Op<Label, User> {
        let read = self.labels.read();
        let rm_ctx = read.derive_rm_ctx();
        self.labels.rm(label, rm_ctx)
    }

    fn contains(&self, label: &Label) -> bool {
        self.labels.contains(label).val
    }
}

pub struct Assignees<User: Actor> {
    actor: User,
    assignees: Orswot<User, User>,
}

impl<User: Actor> Assignees<User> {
    pub fn new(actor: User) -> Self {
        Assignees {
            actor,
            assignees: Orswot::new(),
        }
    }

    pub fn from_assignees(actor: User, assignees: Orswot<User, User>) -> Self {
        Assignees { actor, assignees }
    }
}

impl<User: Actor> AssigneeOp<User, orswot::Op<User, User>> for Assignees<User> {
    fn add(&mut self, user: User) -> orswot::Op<User, User> {
        let read = self.assignees.read();
        let add_ctx = read.derive_add_ctx(self.actor.clone());
        self.assignees.add(user, add_ctx)
    }

    fn remove(&mut self, user: User) -> orswot::Op<User, User> {
        let read = self.assignees.read();
        let rm_ctx = read.derive_rm_ctx();
        self.assignees.rm(user, rm_ctx)
    }

    fn contains(&self, user: &User) -> bool {
        self.assignees.contains(user).val
    }
}

/// A comment of an issue is composed of its [`Comment::author`], the
/// [`Comment::content`] of the comment, and its [`Comment::reactions`].
///
/// It has a unique identifier (of type `Id`) chosen by the implementor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment<Id, User: Actor> {
    identifier: Id,
    author: User,
    content: LWWReg<String, usize>,
    reactions: Orswot<Reaction<User>, User>,
    timestamp: RadClock,
}

impl<Cid, User: Actor> Comment<Cid, User> {
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
            content: LWWReg {
                val: content,
                marker: 0,
            },
            reactions: Orswot::new(),
            timestamp,
        }
    }

    /// Get a reference to to the author of this comment.
    pub fn author(&self) -> &User {
        &self.author
    }

    /// Get a reference to to the content of this comment.
    pub fn content(&self) -> &String {
        &self.content.val
    }

    /// Get the map of reactions to this comment.
    pub fn reactions(&self) -> HashMap<String, Vec<User>>
    where
        User: Clone,
    {
        let mut reaction_map = HashMap::new();
        for reaction in self.reactions.read().val.into_iter() {
            reaction_map
                .entry(reaction.value.clone())
                .and_modify(|users: &mut Vec<User>| users.push(reaction.user.clone()))
                .or_insert_with(|| vec![reaction.user]);
        }
        reaction_map
    }
}

enum COP<User: Actor> {
    ReactionOp(orswot::Op<Reaction<User>, User>),
    EditOp(bool),
}

impl<Id, User: Actor> CommentOp<User, COP<User>> for Comment<Id, User> {
    /// Add a new reaction to the set of reactions on the comment.
    /// Returns `true` if the reaction was new.
    /// Returns `false` if the reaction already existed.
    fn react(&mut self, reaction: Reaction<User>) -> COP<User> {
        let read_ctx = self.reactions.read();
        let add_ctx = read_ctx.derive_add_ctx(reaction.user.clone());
        COP::ReactionOp(self.reactions.add(reaction, add_ctx))
    }

    /// Add a new reaction to the set of reactions on the comment.
    /// Returns `true` if the reaction was in the set and is now removed.
    /// Returns `false` if the reaction was not in the set to begin with.
    fn unreact(&mut self, reaction: Reaction<User>) -> COP<User> {
        let read_ctx = self.reactions.read();
        let rm_ctx = read_ctx.derive_rm_ctx();
        COP::ReactionOp(self.reactions.rm(reaction, rm_ctx))
    }

    fn edit<F: FnOnce(&mut String)>(&mut self, user: User, f: F) -> COP<User> {
        let is_author = self.author == user;
        if is_author {
            let mut new_val = self.content.val.clone();
            f(&mut new_val);
            self.content
                .update(new_val, self.content.marker + 1)
                .expect("Marker is monotonic due to incremenent qed.");
            COP::EditOp(is_author)
        } else {
            COP::EditOp(is_author)
        }
    }
}
