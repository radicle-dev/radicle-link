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
use std::convert::Infallible;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Hide {}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Visibility {
    Visible,
    Hidden,
}

impl Apply for Visibility {
    type Op = Hide;
    type Error = Infallible;

    fn apply(&mut self, _op: Self::Op) -> Result<(), Self::Error> {
        *self = Visibility::Hidden;
        Ok(())
    }
}
