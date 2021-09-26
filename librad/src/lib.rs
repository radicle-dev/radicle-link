// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#![allow(private_intra_doc_links)]
#![warn(clippy::extra_unused_lifetimes)]
#![deny(broken_intra_doc_links)]
#![feature(associated_type_bounds)]
#![feature(backtrace)]
#![feature(bool_to_option)]
#![feature(box_patterns)]
#![feature(btree_drain_filter)]
#![feature(core_intrinsics)]
#![feature(drain_filter)]
#![feature(ip)]
#![feature(never_type)]
#![feature(try_trait_v2)]
#![feature(control_flow_enum)]

#[cfg(feature = "usdt")]
pub mod usdt {
    include!(env!("SONDE_RUST_API_FILE"));
}

#[macro_use]
extern crate async_trait;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate radicle_macros;

pub extern crate radicle_data as data;
pub extern crate radicle_git_ext as git_ext;
pub extern crate radicle_keystore as keystore;
pub extern crate radicle_std_ext as std_ext;

pub mod executor;
pub mod git;
pub mod identities;
pub mod internal;
pub mod keys;
pub mod net;
pub mod paths;
pub mod peer;
pub mod profile;
pub mod rate_limit;
pub mod signer;

// Re-exports
pub use peer::PeerId;
pub use radicle_macros::*;
