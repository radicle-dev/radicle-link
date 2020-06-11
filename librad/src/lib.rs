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

#![feature(str_strip)]
#![feature(bool_to_option)]

extern crate radicle_keystore as keystore;
extern crate sodiumoxide;
#[macro_use]
extern crate lazy_static;

pub use radicle_surf as surf;

pub mod git;
pub mod hash;
pub mod internal;
pub mod keys;
pub mod meta;
pub mod net;
pub mod paths;
pub mod peer;
pub mod uri;

#[cfg(test)]
mod test;
