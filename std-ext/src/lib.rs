// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#![cfg_attr(feature = "nightly", feature(try_trait_v2))]

pub mod ops;
pub mod result;

pub type Void = std::convert::Infallible;

pub mod prelude {
    use super::*;

    pub use super::Void;
    pub use ops::{FromResidual, Try};
    pub use result::ResultExt;
}
