// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#![feature(never_type)]

#[macro_use]
extern crate lazy_static;

pub mod git;
pub mod logging;
pub mod rad;
pub mod roundtrip;
pub mod tempdir;
