// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

pub mod delegation;
pub mod generic;
pub mod git;
pub mod payload;
pub mod relations;
pub mod sign;
pub mod urn;
pub use urn::Urn;
pub mod xor;

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
