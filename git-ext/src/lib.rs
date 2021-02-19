// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! Extensions and wrappers for `git2` types

pub mod blob;
pub mod error;
pub mod oid;
pub mod reference;
pub mod revwalk;
pub mod transport;
pub mod tree;

pub use blob::*;
pub use error::*;
pub use oid::*;
pub use reference::*;
pub use revwalk::*;
pub use transport::*;
pub use tree::Tree;

#[cfg(test)]
#[macro_use]
extern crate assert_matches;

#[cfg(test)]
#[macro_use]
extern crate lazy_static;
