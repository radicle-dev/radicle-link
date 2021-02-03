// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#![feature(never_type)]

pub mod error;
pub use error::Error;

pub mod cli;
pub mod garden;
pub mod include;
pub use garden::{graft, plant, repot};

mod git;
mod sealed;
