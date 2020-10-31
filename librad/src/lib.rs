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

#![allow(incomplete_features)]
#![warn(clippy::extra_unused_lifetimes)]
#![feature(associated_type_bounds)]
#![feature(backtrace)]
#![feature(bool_to_option)]
#![feature(btree_drain_filter)]
#![feature(core_intrinsics)]
#![feature(generic_associated_types)]
#![feature(never_type)]

#[macro_use]
extern crate async_trait;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate radicle_macros;

pub extern crate radicle_git_ext as git_ext;
pub extern crate radicle_keystore as keystore;
pub extern crate radicle_std_ext as std_ext;

pub mod git;
pub mod identities;
pub mod internal;
pub mod keys;
pub mod net;
pub mod paths;
pub mod peer;
pub mod signer;

// Re-exports
pub use radicle_macros::*;

#[cfg(test)]
mod test;

#[cfg(test)]
#[macro_use]
extern crate futures_await_test;
#[cfg(test)]
#[macro_use]
extern crate assert_matches;
