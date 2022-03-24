// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::Seed;

pub mod file;
pub use file::{FileStore, Iter};

/// Get an iterator of the [`Seed`] in the [`Store`].
pub trait Store {
    type Scan: std::error::Error + Send + Sync + 'static;
    type Iter: std::error::Error + Send + Sync + 'static;

    type Addrs;
    type Seeds: Iterator<Item = Result<Seed<Self::Addrs>, Self::Iter>>;

    /// Retrieve all [`Seed`]s in the storage.
    ///
    /// Seeds are expected to be in the following format:
    /// ```text
    /// <peer id>@<addr>[,<label>]
    /// ```
    fn scan(&self) -> Result<Self::Seeds, Self::Scan>;
}
