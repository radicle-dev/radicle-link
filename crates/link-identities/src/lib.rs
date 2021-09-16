// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#![allow(private_intra_doc_links)]
#![warn(clippy::extra_unused_lifetimes)]
#![deny(broken_intra_doc_links)]
#![feature(bool_to_option, never_type)]

use std::fmt::{self, Display};

pub extern crate link_canonical as canonical;
pub extern crate link_crypto as crypto;

#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate radicle_macros;

extern crate radicle_data as data;
extern crate radicle_git_ext as git_ext;
extern crate radicle_std_ext as std_ext;

pub mod delegation;
pub mod generic;
pub mod git;
pub mod payload;
pub mod relations;
pub mod sign;

pub mod urn;
pub use urn::Urn;

pub mod xor;
pub use xor::Xor;

mod sealed;

pub use git::*;

#[derive(Clone, Debug, minicbor::Encode, minicbor::Decode)]
pub enum SomeUrn {
    #[n(0)]
    Git(#[n(0)] git::Urn),
}

impl From<git::Urn> for SomeUrn {
    fn from(urn: git::Urn) -> Self {
        Self::Git(urn)
    }
}

impl Display for SomeUrn {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let Self::Git(urn) = self;
        write!(f, "{}", urn)
    }
}
