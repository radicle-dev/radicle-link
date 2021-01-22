// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::hash::Hash;

pub mod delegation;
pub mod generic;
pub mod git;
pub mod payload;
pub mod sign;
pub mod urn;
pub use urn::Urn;

mod sealed;

#[cfg(test)]
pub(crate) mod gen;

pub use git::*;

#[derive(Clone, Debug, Hash, minicbor::Encode, minicbor::Decode)]
pub enum SomeUrn {
    #[n(0)]
    Git(#[n(0)] git::Urn),
}
