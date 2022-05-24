// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeSet;

use bstr::ByteSlice as _;
use link_crypto::PeerId;
use link_git::protocol::{oid, Ref};
use radicle_data::NonEmptyVec;

use crate::{
    error,
    ids,
    internal::{self, Layout, UpdateTips},
    peek,
    prepare,
    refs,
    sigrefs,
    state::FetchState,
    transmit,
    FilteredRef,
    Identities,
    LsRefs,
    Negotiation,
    Odb,
    RefPrefix,
    RefScan,
    Refdb,
    WantsHaves,
};

/// Fetching of transitively tracked trees.
///
/// [`super::Fetch`] may discover sigrefs corresponding the peers in the
/// transitive tracking graph. Those trees then need to be fetched in a separate
/// operation. Since transitive trees need to be verified, too, the
/// [`Transitive`] phase requests verification refs in addition to the signed
/// refs. That is, it combines the peek and fetch phases conducted
/// earlier, constrained to the set of transitively tracked peers.
#[derive(Debug)]
pub struct Transitive<Oid> {
    /// The local id.
    pub local_id: PeerId,
    /// The peer being fetched from.
    pub remote_id: PeerId,
    /// The signed refs discovered by [`super::Fetch`], descibing the refs
    /// we'll ask for.
    pub signed_refs: sigrefs::Combined<Oid>,
    /// Trasitively tracked peers for which [`super::Fetch`] discovered
    /// `rad/id`s for.
    pub identities: BTreeSet<PeerId>,
    /// Transitively tracked peers for which the [`crate::DataPolicy`] is
    /// [`crate::DataPolicy::Deny`]. Must not appear in `signed_refs`.
    ///
    /// Used to determine whether to fetch verification refs regardless of
    /// policy.
    pub denied: BTreeSet<PeerId>,
    /// The maximum number of bytes the fetched packfile can have.
    pub limit: u64,
}

impl<Oid> Transitive<Oid> {
    fn ref_prefixes(&self) -> impl Iterator<Item = RefPrefix> + '_ {
        self.signed_refs
            .keys()
            .chain(&self.denied)
            .flat_map(move |id| peek::ref_prefixes(id, &self.remote_id))
            .chain(
                self.identities
                    .iter()
                    .flat_map(move |id| peek::ref_prefixes(id, &self.remote_id)),
            )
    }

    fn required_refs(&self) -> impl Iterator<Item = refs::Scoped<'_, 'static>> {
        self.signed_refs
            .keys()
            .flat_map(move |id| peek::required_refs(id, &self.remote_id))
            .chain(
                self.denied
                    .iter()
                    .map(move |id| refs::scoped(id, &self.remote_id, refs::Owned::refs_rad_id())),
            )
    }
}

impl<T: AsRef<oid>> Negotiation for Transitive<T> {
    fn ls_refs(&self) -> Option<LsRefs> {
        NonEmptyVec::from_vec(self.ref_prefixes().collect()).map(LsRefs::from)
    }

    fn ref_filter(&self, r: Ref) -> Option<FilteredRef<Self>> {
        use either::Either::*;
        use refs::parsed::Identity;

        let (name, tip) = refs::into_unpacked(r);
        let parsed = refs::parse::<Identity>(name.as_bstr()).ok()?;
        match parsed {
            refs::Parsed {
                remote: Some(remote_id),
                inner: Left(ref rad),
            } if self.signed_refs.contains_key(&remote_id) || self.denied.contains(&remote_id) => {
                // To maintain consistency between signed refs and the refs they
                // point to, `Fetch` can't include the signed_refs ref in the
                // transaction. In principle, we could inject it in
                // `Transitive::prepare`, but that gets us into ownership
                // trouble.
                //
                // Thus, request signed_refs in ls-refs again, but ensure here
                // that we use the tip seen previously (the remote could have
                // changed between `Fetch` and `Transitive`). `pre_validate`
                // must ensure that we have seen signed_refs.
                let tip = match rad {
                    refs::parsed::Rad::SignedRefs => self
                        .signed_refs
                        .get(&remote_id)
                        .map(|sig| sig.at.as_ref().to_owned())?,
                    _ => tip,
                };
                Some(FilteredRef::new(tip, &remote_id, parsed))
            },

            refs::Parsed {
                remote: Some(remote_id),
                inner: Left(refs::parsed::Rad::Id | refs::parsed::Rad::Ids { .. }),
            } => Some(FilteredRef::new(tip, &remote_id, parsed)),

            _ => None,
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
        super::wants_haves(db, refs, self.signed_refs.iter().map(|(x, y)| (x, &y.refs)))
    }

    fn fetch_limit(&self) -> u64 {
        self.limit
    }
}

impl<T: AsRef<oid>> UpdateTips for Transitive<T> {
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
        prepare::verification_refs(&self.local_id, s, cx, refs, |_remote_id| false)
    }
}

impl<T> Layout for Transitive<T> {
    fn pre_validate(&self, refs: &[FilteredRef<Self>]) -> Result<(), error::Layout> {
        peek::guard_required(
            self.required_refs().collect(),
            refs.iter()
                .map(|x| refs::scoped(x.remote_id(), &self.remote_id, x.to_owned()))
                .collect(),
        )
    }
}
