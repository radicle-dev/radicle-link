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

#[macro_use]
extern crate async_trait;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate radicle_macros;

pub extern crate link_canonical as canonical;
pub extern crate link_crypto as crypto;
pub extern crate link_identities as identities;
pub extern crate radicle_data as data;
pub extern crate radicle_git_ext as git_ext;
pub extern crate radicle_std_ext as std_ext;

pub mod git;
pub mod internal;
pub mod net;
pub mod paths;
pub mod profile;
pub mod rate_limit;

// Re-exports
pub use link_crypto::{keystore, PeerId, PublicKey, SecStr, SecretKey, Signature, Signer};
pub use radicle_macros::*;
