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
    metadata::Label,
    ops::metadata::{AssigneeOp, LabelOp},
};

use crdts::{
    self,
    orswot::{self, Orswot},
    vclock::Actor,
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
