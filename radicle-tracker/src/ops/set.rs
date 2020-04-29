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

use crate::ops::Apply;
use std::{collections::HashSet, convert::Infallible, hash::Hash};

pub enum Op<A> {
    Insert(A),
    Remove(A),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Set<A: Eq + Hash>(pub(crate) HashSet<A>);

impl<A: Eq + Hash> Set<A> {
    pub fn new() -> Self {
        Set(HashSet::new())
    }

    pub fn insert(&mut self, a: A) -> Op<A>
    where
        A: Clone,
    {
        self.0.insert(a.clone());
        Op::Insert(a)
    }

    pub fn remove(&mut self, a: A) -> Op<A> {
        self.0.remove(&a);
        Op::Remove(a)
    }
}

impl<A: Eq + Hash> Apply for Set<A> {
    type Op = Op<A>;
    type Error = Infallible;

    fn apply(&mut self, op: Self::Op) -> Result<(), Self::Error> {
        match op {
            Op::Insert(a) => self.0.insert(a),
            Op::Remove(a) => self.0.remove(&a),
        };

        Ok(())
    }
}
