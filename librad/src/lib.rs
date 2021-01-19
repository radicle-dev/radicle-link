// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#![allow(incomplete_features)]
#![allow(private_intra_doc_links)]
#![warn(clippy::extra_unused_lifetimes)]
#![feature(associated_type_bounds)]
#![feature(backtrace)]
#![feature(bool_to_option)]
#![feature(btree_drain_filter)]
#![feature(core_intrinsics)]
#![feature(generic_associated_types)]
#![feature(ip)]
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
pub mod profile;
pub mod signer;

// Re-exports
pub use radicle_macros::*;

#[cfg(test)]
#[macro_use]
extern crate futures_await_test;
#[cfg(test)]
#[macro_use]
extern crate assert_matches;
