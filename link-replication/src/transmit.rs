// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::{BTreeSet, HashSet},
    hash::{Hash, Hasher},
    marker::PhantomData,
};

use bstr::BString;
use either::Either;
use link_crypto::PeerId;
use link_git::protocol::{ObjectId, Ref};
use thiserror::Error;

use crate::{refs, Refdb};

#[derive(Debug, Error)]
pub enum SkippedFetch {
    #[error("remote did not advertise any matching refs")]
    NoMatchingRefs,
    #[error("all local refs up-to-date")]
    WantNothing,
}

#[async_trait(?Send)]
pub trait Net {
    type Error: std::error::Error + Send + Sync + 'static;

    async fn run_fetch<N, T>(
        &self,
        neg: N,
    ) -> Result<(N, Result<Vec<FilteredRef<T>>, SkippedFetch>), Self::Error>
    where
        N: Negotiation<T> + Send,
        T: Send + 'static;
}

pub trait Negotiation<T = Self> {
    /// The `ref-prefix`es to send with `ls-refs`.
    fn ref_prefixes(&self) -> Vec<refs::Scoped<'_, '_>>;

    /// Filter a remote-advertised [`Ref`].
    ///
    /// Return `Some` if the ref should be considered, `None` otherwise. This
    /// method may be called with the response of `ls-refs`, the `wanted-refs`
    /// of a `fetch` response, or both.
    fn ref_filter(&self, r: Ref) -> Option<FilteredRef<T>>;

    /// Assemble the `want`s and `have`s for a `fetch`, retaining the refs which
    /// would need updating after the `fetch` succeeds.
    ///
    /// The `refs` are the advertised refs from executing `ls-refs`, filtered
    /// through [`Negotiation::ref_filter`].
    fn wants_haves<R: Refdb>(
        &self,
        db: &R,
        refs: impl IntoIterator<Item = FilteredRef<T>>,
    ) -> Result<WantsHaves<T>, R::FindError>;

    /// Maximum number of bytes the fetched packfile is allowed to have.
    fn fetch_limit(&self) -> u64;
}

pub struct WantsHaves<T: ?Sized> {
    pub wanted: HashSet<FilteredRef<T>>,
    pub wants: BTreeSet<ObjectId>,
    pub haves: BTreeSet<ObjectId>,
}

pub struct FilteredRef<T: ?Sized> {
    pub name: BString,
    pub tip: ObjectId,
    pub remote_id: PeerId,
    pub parsed: Either<refs::parsed::Rad<refs::parsed::Identity>, refs::parsed::Refs>,
    _marker: PhantomData<T>,
}

impl<T> FilteredRef<T> {
    #[allow(clippy::unnecessary_lazy_evaluations)]
    pub fn new(
        name: BString,
        tip: ObjectId,
        remote_id: &PeerId,
        parsed: refs::Parsed<refs::parsed::Identity>,
    ) -> Self {
        Self {
            name,
            tip,
            remote_id: parsed.remote.unwrap_or_else(|| *remote_id),
            parsed: parsed.inner,
            _marker: PhantomData,
        }
    }
}

impl<T> Clone for FilteredRef<T> {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            tip: self.tip,
            remote_id: self.remote_id,
            parsed: self.parsed.clone(),
            _marker: PhantomData,
        }
    }
}

impl<T> PartialEq for FilteredRef<T> {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.tip == other.tip && self.remote_id == other.remote_id
    }
}

impl<T> Eq for FilteredRef<T> {}

impl<T> Hash for FilteredRef<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.tip.hash(state);
        self.remote_id.hash(state);
    }
}
