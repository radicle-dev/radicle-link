// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#![feature(bool_to_option)]
#![feature(never_type)]
#![feature(assert_matches)]
#![feature(path_try_exists)]

#[macro_use]
extern crate lazy_static;
#[cfg(test)]
#[macro_use]
extern crate nonzero_ext;
#[cfg(test)]
#[macro_use]
extern crate futures_await_test;
#[cfg(test)]
#[macro_use]
extern crate link_canonical;

#[macro_use]
pub mod daemon;
pub mod canonical;
pub mod git;
pub mod librad;
pub mod link_async;
pub mod logging;
pub mod rad;
pub mod roundtrip;
pub mod ssh;
pub mod tempdir;

#[cfg(test)]
mod test;
