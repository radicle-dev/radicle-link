// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use crate::{error, ids, track, FetchState, FilteredRef, Identities, RefScan, Update};

pub(crate) struct Updates<'a, U> {
    pub tips: Vec<Update<'a>>,
    pub track: Vec<track::Rel<U>>,
}

pub(crate) trait UpdateTips<T = Self> {
    fn prepare<'a, U, C>(
        &self,
        s: &FetchState<U>,
        cx: &C,
        refs: &'a [FilteredRef<T>],
    ) -> Result<Updates<'a, U>, error::Prepare>
    where
        U: ids::Urn + Ord,
        C: Identities<Urn = U>,
        for<'b> &'b C: RefScan;
}

pub(crate) trait Layout<T = Self> {
    /// Validate that all advertised refs conform to an expected layout.
    ///
    /// The supplied `refs` are `ls-ref`-advertised refs filtered through
    /// [`crate::Negotiation::ref_filter`].
    fn pre_validate(&self, refs: &[FilteredRef<T>]) -> Result<(), error::Layout>;
}
