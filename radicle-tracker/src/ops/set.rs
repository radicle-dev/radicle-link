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

use std::{collections::HashSet, hash::Hash};

pub enum SetOp<A> {
    Insert(A),
    Remove(A),
}

#[derive(Debug, Clone)]
pub struct Set<A: Eq + Hash>(HashSet<A>);

impl<A: Eq + Hash> Set<A> {
    pub fn insert(&mut self, a: A) -> SetOp<A>
    where
        A: Clone,
    {
        self.0.insert(a.clone());
        SetOp::Insert(a)
    }

    pub fn remove(&mut self, a: A) -> SetOp<A> {
        self.0.remove(&a);
        SetOp::Remove(a)
    }

    pub fn apply(&mut self, op: SetOp<A>) -> bool {
        match op {
            SetOp::Insert(a) => self.0.insert(a),
            SetOp::Remove(a) => self.0.remove(&a),
        }
    }
}
