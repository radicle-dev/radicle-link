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

use crate::ops::{
    visibility::{Hide, Visibility},
    Apply,
    absurd,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Item<A> {
    pub val: A,
    pub visibility: Visibility,
}

impl<A> Item<A> {
    pub fn new(val: A) -> Self {
        Item {
            val,
            visibility: Visibility::Visible,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Modifier<E> {
    Edit(E),
    Delete(Hide),
}

impl<E, A: Apply<Op = E>> Apply for Item<A> {
    type Op = Modifier<E>;
    type Error = A::Error;

    fn apply(&mut self, op: Self::Op) -> Result<(), Self::Error> {
        match op {
            Modifier::Edit(e) => self.val.apply(e),
            Modifier::Delete(h) => self.visibility.apply(h).map_err(absurd),
        }
    }
}
