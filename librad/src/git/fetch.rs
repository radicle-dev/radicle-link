// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::BTreeMap, ops::Deref};

use git_ext as ext;

use crate::identities::Urn;

mod specs;
pub use specs::Fetchspecs;

/// 1KiB for use in [`Limit`] combinations.
pub const ONE_KB: usize = 1024;
/// 5KiB for use in [`Limit`], specifically for the `peek` field, when we would
/// like to fetch `rad/id` , `rad/self`, `rad/ids/*` references.
pub const TEN_KB: usize = ONE_KB * 10;
/// 5GB for use in [`Limit`], specifically for the `data` field, when we would
/// like to fetch `rad/*` as well as `refs/heads/*` references.
pub const FIVE_GB: usize = ONE_KB * ONE_KB * ONE_KB * 5;

/// Limits used for guarding against fetching large amounts of data from the
/// network.
///
/// The default values are [`TEN_KB`], [`FIVE_GB`], respectively.
#[derive(Clone, Copy, Debug)]
pub struct Limit {
    /// Limit the amount of data we fetch using [`Fetchspecs::PeekAll`] and
    /// [`Fetchspecs::Peek`].
    pub peek: usize,
    /// Limit the amount of data we fetch using [`Fetchspecs::Replicate`].
    pub data: usize,
}

impl Default for Limit {
    fn default() -> Self {
        Self {
            peek: TEN_KB,
            data: FIVE_GB,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RemoteHeads(BTreeMap<ext::RefLike, ext::Oid>);

impl Deref for RemoteHeads {
    type Target = BTreeMap<ext::RefLike, ext::Oid>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<BTreeMap<ext::RefLike, ext::Oid>> for RemoteHeads {
    fn from(map: BTreeMap<ext::RefLike, ext::Oid>) -> Self {
        Self(map)
    }
}

pub struct FetchResult {
    pub updated_tips: BTreeMap<ext::RefLike, ext::Oid>,
}

/// Types which can process [`Fetchspecs`], and update the local storage
/// accordingly.
pub trait Fetcher {
    type Error;
    type PeerId;
    type UrnId;

    /// The [`Urn`] this fetcher is fetching.
    fn urn(&self) -> &Urn<Self::UrnId>;

    /// The remote peer this fetcher is fetching from.
    fn remote_peer(&self) -> &Self::PeerId;

    /// The [`RemoteHeads`] the remote end advertised.
    fn remote_heads(&self) -> &RemoteHeads;

    /// Fetch the given [`Fetchspecs`].
    fn fetch(
        &mut self,
        fetchspecs: Fetchspecs<Self::PeerId, Self::UrnId>,
    ) -> Result<FetchResult, Self::Error>;
}
