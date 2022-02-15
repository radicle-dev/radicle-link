// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

mod check;
pub use check::{ref_format as check_ref_format, Error, Options};

mod deriv;
pub use deriv::{Namespaced, Qualified};

pub mod lit;

pub mod name;
#[cfg(feature = "percent-encoding")]
pub use name::PercentEncode;
pub use name::{Component, RefStr, RefString};

pub mod refspec;
pub use refspec::DuplicateGlob;

#[cfg(feature = "minicbor")]
mod cbor;
#[cfg(feature = "serde")]
mod serde;
