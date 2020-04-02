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

use crate::metadata::Label;

pub trait LabelOp<Op> {
    fn add(&mut self, label: Label) -> Op;
    fn remove(&mut self, label: Label) -> Op;
    fn contains(&self, label: &Label) -> bool;
}

pub trait AssigneeOp<User, Op> {
    fn add(&mut self, user: User) -> Op;
    fn remove(&mut self, user: User) -> Op;
    fn contains(&self, user: &User) -> bool;
}
