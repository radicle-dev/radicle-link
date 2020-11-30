// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

mod iter;
pub use iter::{ReferenceNames, References};

pub mod name;
pub use name::{OneLevel, Qualified, RefLike, RefspecPattern};
