// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#![feature(array_map, never_type)]

#[macro_use]
extern crate async_trait;

pub mod protocol;

pub use git_actor as actor;
pub use git_hash as hash;
pub use git_lock as lock;
pub use git_object as object;
pub use git_ref as refs;
pub use git_traverse as traverse;
