// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! Abstractions and utilities to run and interact with link and surf.

#![warn(
    clippy::all,
    clippy::cargo,
    clippy::nursery,
    clippy::pedantic,
    clippy::unwrap_used,
    missing_docs,
    unused_import_braces,
    unused_qualifications
)]
#![allow(
    clippy::cargo_common_metadata,
    clippy::clone_on_ref_ptr,
    clippy::expect_used,
    clippy::implicit_return,
    clippy::integer_arithmetic,
    clippy::missing_inline_in_public_items,
    clippy::multiple_crate_versions,
    clippy::multiple_inherent_impl,
    clippy::similar_names,
    clippy::too_many_lines
)]
#![feature(duration_zero, hash_set_entry, never_type)]

#[cfg(test)]
extern crate assert_matches;

pub use librad::{
    self,
    git::{self, identities::local::LocalIdentity, include, local::url::LocalUrl, Urn},
    identities::{self, Person, Project},
    keys,
    net::{self, discovery},
    paths::Paths,
    peer::PeerId,
    profile,
    signer,
};

pub use librad::git_ext;

pub use radicle_git_helpers::remote_helper;

pub mod config;
pub mod convert;
pub mod keystore;
pub mod peer;
pub use peer::{Control as PeerControl, Event as PeerEvent, Peer, RunConfig, Status as PeerStatus};
pub mod patch;
pub mod project;
pub mod request;
pub mod state;

pub mod seed;
