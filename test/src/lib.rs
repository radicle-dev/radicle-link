// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#[cfg(test)]
#[macro_use]
extern crate assert_matches;
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
extern crate radicle_std_ext as std_ext;

#[macro_use]
pub mod daemon;
pub mod canonical;
pub mod git;
pub mod librad;
pub mod link_async;
#[cfg(test)]
pub mod node_lib;

#[cfg(test)]
mod test;
