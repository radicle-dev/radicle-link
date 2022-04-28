// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::collections::BTreeMap;

use bstr::ByteSlice;
use link_crypto::PeerId;
use link_git::protocol::Ref;
use radicle_data::NonEmptyVec;

use super::{guard_required, ref_prefixes, required_refs};
use crate::{
    error,
    ids,
    internal::{self, Layout, UpdateTips},
    prepare,
    refs,
    track,
    transmit::{self, BuildWantsHaves, LsRefs},
    FetchState,
    FilteredRef,
    Identities,
    Negotiation,
    Odb,
    RefPrefix,
    RefScan,
    Refdb,
    WantsHaves,
};

#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub is_delegate: bool,
    pub policy: track::DataPolicy,
}

#[derive(Debug)]
pub struct ForFetch {
    /// The local peer, so we don't fetch our own data.
    pub local_id: PeerId,
    /// The remote peer being fetched from.
    pub remote_id: PeerId,
    /// The set of tracked peers, including delegates.
    pub tracked: BTreeMap<PeerId, Spec>,
    /// Maximum number of bytes the fetched packfile is allowed to have.
    pub limit: u64,
}

impl ForFetch {
    pub fn peers(&self) -> impl Iterator<Item = &PeerId> {
        self.tracked.keys().filter(move |id| *id != &self.local_id)
    }

    pub fn required_refs(&self) -> impl Iterator<Item = refs::Scoped<'_, 'static>> {
        self.tracked
            .iter()
            .filter(move |(id, spec)| *id != &self.local_id && spec.is_delegate)
            .flat_map(move |(id, _)| required_refs(id, &self.remote_id))
    }

    fn ref_prefixes(&self) -> impl Iterator<Item = RefPrefix> + '_ {
        self.peers()
            .flat_map(move |id| ref_prefixes(id, &self.remote_id))
    }
}

impl Negotiation for ForFetch {
    fn ls_refs(&self) -> Option<LsRefs> {
        NonEmptyVec::from_vec(self.ref_prefixes().collect()).map(LsRefs::from)
    }

    fn ref_filter(&self, r: Ref) -> Option<FilteredRef<Self>> {
        use refs::parsed::Identity;

        let (name, tip) = refs::into_unpacked(r);
        let parsed = refs::parse::<Identity>(name.as_bstr()).ok()?;
        let remote_id = match &parsed.remote {
            Some(remote_id) if remote_id == &self.local_id => None,
            Some(remote_id) => Some(*remote_id),
            None => Some(self.remote_id),
        }?;
        let policy = self.tracked.get(&remote_id).map(|spec| spec.policy)?;
        match parsed.inner.as_ref().left()? {
            refs::parsed::Rad::SignedRefs => match policy {
                track::DataPolicy::Deny => None,
                track::DataPolicy::Allow => Some(FilteredRef::new(tip, &remote_id, parsed)),
            },

            _ => Some(FilteredRef::new(tip, &remote_id, parsed)),
        }
    }

    fn wants_haves<R>(
        &self,
        db: &R,
        refs: &[FilteredRef<Self>],
    ) -> Result<Option<WantsHaves>, transmit::error::WantsHaves<R::FindError>>
    where
        R: Refdb + Odb,
    {
        let mut bld = BuildWantsHaves::default();
        bld.add(db, refs)?;
        Ok(bld.build())
    }

    fn fetch_limit(&self) -> u64 {
        self.limit
    }
}

impl UpdateTips for ForFetch {
    fn prepare<'a, U, C>(
        &self,
        s: &FetchState<U>,
        cx: &C,
        refs: &'a [FilteredRef<Self>],
    ) -> Result<internal::Updates<'a, U>, error::Prepare>
    where
        U: ids::Urn + Ord,
        C: Identities<Urn = U>,
        for<'b> &'b C: RefScan,
    {
        prepare::verification_refs(&self.local_id, s, cx, refs, |remote_id| {
            self.tracked
                .get(remote_id)
                .expect("`ref_filter` yields only tracked refs")
                .is_delegate
        })
    }
}

impl Layout for ForFetch {
    fn pre_validate(&self, refs: &[FilteredRef<Self>]) -> Result<(), error::Layout> {
        guard_required(
            self.required_refs().collect(),
            refs.iter()
                .map(|x| refs::scoped(x.remote_id(), &self.remote_id, x.to_owned()))
                .collect(),
        )
    }
}
