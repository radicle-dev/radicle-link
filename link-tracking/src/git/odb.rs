// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use crate::git::config::Config;

pub trait Read {
    type FindError: std::error::Error + Send + Sync + 'static;

    type Oid;

    /// Find and parse the [`Config`] that is expected to be found at the given
    /// `oid`. If no object was found for `oid`, then the result should be
    /// `None`.
    ///
    /// This is expected to load a blob from the backing object database and
    /// parse the contents of that blob into the `Config`.
    fn find_config(&self, oid: &Self::Oid) -> Result<Option<Config>, Self::FindError>;
}

pub trait Write {
    type ModifyError: std::error::Error + Send + Sync + 'static;
    type WriteError: std::error::Error + Send + Sync + 'static;

    type Oid;

    /// Write the [`Config`] to the backing object database.
    ///
    /// This is expected to serialise the [`Config`] to Canonical JSON bytes and
    /// write them as a blob. See [`link_canonical::Canonical`] for
    /// serialisation.
    fn write_config(&self, config: &Config) -> Result<Self::Oid, Self::WriteError>;

    /// Modify the [`Config`] found at `oid`.
    ///
    /// This expected to check if the [`Config`] exists and serialise the new
    /// [`Config`] to Canonical JSON bytes, writing them as a blob. See
    /// [`link_canonical::Canonical`] for serialisation.
    fn modify_config<F>(&self, oid: &Self::Oid, f: F) -> Result<Self::Oid, Self::ModifyError>
    where
        F: FnOnce(Config) -> Config;
}
