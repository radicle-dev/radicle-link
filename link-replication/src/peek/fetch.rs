// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::{BTreeSet, HashSet},
    convert::TryFrom,
};

use bstr::ByteVec as _;
use git_ref_format::RefString;
use link_crypto::PeerId;
use link_git::protocol::{ObjectId, Ref};

use super::{guard_required, mk_ref_update, ref_prefixes, required_refs};
use crate::{
    error,
    ids,
    internal::{self, Layout, UpdateTips},
    refdb,
    refs,
    track,
    FetchState,
    FilteredRef,
    Identities,
    Negotiation,
    RefPrefix,
    Refdb,
    Update,
    WantsHaves,
};

#[derive(Debug)]
pub struct ForFetch {
    /// The local peer, so we don't fetch our own data.
    pub local_id: PeerId,
    /// The remote peer being fetched from.
    pub remote_id: PeerId,
    /// The set of keys the latest known identity revision delegates to.
    /// Indirect delegations are resolved.
    pub delegates: BTreeSet<PeerId>,
    /// Additional peers being tracked (ie. excluding `delegates`).
    pub tracked: BTreeSet<PeerId>,
    /// Maximum number of bytes the fetched packfile is allowed to have.
    pub limit: u64,
}

impl ForFetch {
    pub fn peers(&self) -> impl Iterator<Item = &PeerId> {
        self.delegates
            .iter()
            .chain(self.tracked.iter())
            .filter(move |id| *id != &self.local_id)
    }

    pub fn required_refs(&self) -> impl Iterator<Item = refs::Scoped<'_, 'static>> {
        self.delegates
            .iter()
            .filter(move |id| *id != &self.local_id)
            .flat_map(move |id| required_refs(id, &self.remote_id))
    }
}

impl Negotiation for ForFetch {
    fn ref_prefixes(&self) -> Vec<RefPrefix> {
        self.peers()
            .flat_map(move |id| ref_prefixes(id, &self.remote_id))
            .collect()
    }

    fn ref_filter(&self, r: Ref) -> Option<FilteredRef<Self>> {
        use refs::parsed::Identity;

        let (name, tip) = refs::into_unpacked(r);
        let name = {
            let s = Vec::from(name).into_string().ok()?;
            RefString::try_from(s).ok()?
        };
        // FIXME:: precompute / memoize `Self::ref_prefixes`
        if !self
            .ref_prefixes()
            .iter()
            .any(|prefix| prefix.matches(&name))
        {
            return None;
        }
        let parsed = refs::parse::<Identity>(name.as_bstr()).ok()?;

        match parsed.remote {
            Some(remote_id) if remote_id == self.local_id => None,
            Some(remote_id) => Some(FilteredRef::new(tip, &remote_id, parsed)),
            None => Some(FilteredRef::new(tip, &self.remote_id, parsed)),
        }
    }

    fn wants_haves<R: Refdb>(
        &self,
        db: &R,
        refs: impl IntoIterator<Item = FilteredRef<Self>>,
    ) -> Result<WantsHaves<Self>, R::FindError> {
        let mut wanted = HashSet::new();
        let mut wants = BTreeSet::new();
        let mut haves = BTreeSet::new();

        for r in refs {
            let refname = refs::Qualified::from(r.to_remote_tracking());
            match db.refname_to_id(refname)? {
                Some(oid) => {
                    if oid.as_ref() != r.tip {
                        wants.insert(r.tip);
                    }
                    haves.insert(oid.into());
                },
                None => {
                    wants.insert(r.tip);
                },
            }
            wanted.insert(r);
        }

        Ok(WantsHaves {
            wanted,
            wants,
            haves,
        })
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
    ) -> Result<internal::Updates<'a, U>, error::Prepare<C::VerificationError, C::FindError>>
    where
        U: ids::Urn + Ord,
        C: Identities<Urn = U> + Refdb,
    {
        use ids::VerifiedIdentity as _;
        use refdb::{Policy, SymrefTarget};

        let mut tips = Vec::new();
        let mut track = Vec::new();
        for r in refs {
            debug_assert!(r.remote_id() != &self.local_id, "never touch our own");
            let is_delegate = self.delegates.contains(r.remote_id());
            // symref `rad/self` if we already have the top-level identity
            if r.is(&refs::parsed::Rad::Me) {
                match Identities::verify(cx, r.tip, |_| None::<ObjectId>) {
                    Err(e) if is_delegate => return Err(error::Prepare::Verification(e)),
                    Err(e) => warn!("invalid `rad/self`: {}", e),
                    Ok(id) => {
                        let top = refs::namespaced(&id.urn(), refs::REFS_RAD_ID);
                        let top_qual = top.clone().into_qualified();
                        let oid = Refdb::refname_to_id(cx, &top_qual).map_err(|source| {
                            error::Prepare::FindRef {
                                name: top_qual.into_refstring(),
                                source,
                            }
                        })?;
                        let track_as = r.to_remote_tracking();
                        let up = match oid {
                            Some(oid) => Update::Symbolic {
                                name: track_as.into(),
                                target: SymrefTarget {
                                    name: top,
                                    target: oid.as_ref().to_owned(),
                                },
                                type_change: Policy::Allow,
                            },
                            None => Update::Direct {
                                name: track_as.into(),
                                target: r.tip,
                                no_ff: Policy::Abort,
                            },
                        };

                        tips.push(up);
                        track.push(track::Rel::SelfRef(id.urn()));
                    },
                }
            } else {
                // XXX: we should verify all ids at some point, but non-delegates
                // would be a warning only
                if is_delegate && r.is(&refs::parsed::Rad::Id) {
                    Identities::verify(cx, r.tip, s.lookup_delegations(r.remote_id()))
                        .map_err(error::Prepare::Verification)?;
                }
                if let Some(u) = mk_ref_update::<_, C::Urn>(r) {
                    tips.push(u)
                }
            }
        }

        Ok(internal::Updates { tips, track })
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
