// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    convert::TryFrom,
};

use bstr::ByteVec as _;
use git_ref_format::{Qualified, RefString};
use link_crypto::PeerId;
use link_git::protocol::{ObjectId, Ref};
use radicle_data::NonEmptyVec;

use super::{guard_required, mk_ref_update, ref_prefixes, required_refs};
use crate::{
    error,
    ids,
    internal::{self, Layout, UpdateTips},
    refdb,
    refs,
    track,
    transmit::{self, ExpectLs, LsRefs},
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

    fn ref_prefixes(&self) -> impl Iterator<Item = RefPrefix> + '_ {
        self.peers()
            .flat_map(move |id| ref_prefixes(id, &self.remote_id))
    }
}

impl Negotiation for ForFetch {
    fn ls_refs(&self) -> Option<LsRefs> {
        let prefixes = self.ref_prefixes();
        NonEmptyVec::from_vec(prefixes.collect()).map(|prefixes| LsRefs::Prefix {
            prefixes,
            response: ExpectLs::NonEmpty,
        })
    }

    fn ref_filter(&self, r: Ref) -> Option<FilteredRef<Self>> {
        use refs::parsed::Identity;

        let (name, tip) = refs::into_unpacked(r);
        let name = {
            let s = Vec::from(name).into_string().ok()?;
            RefString::try_from(s).ok()?
        };
        // FIXME:: precompute / memoize `Self::ref_prefixes`
        if !self.ref_prefixes().any(|prefix| prefix.matches(&name)) {
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
    ) -> Result<WantsHaves<Self>, transmit::error::WantsHaves<R::FindError>> {
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
    ) -> Result<internal::Updates<'a, U>, error::Prepare>
    where
        U: ids::Urn + Ord,
        C: Identities<Urn = U>,
    {
        use either::Either::*;
        use ids::VerifiedIdentity as _;

        let grouped = refs
            .iter()
            .filter_map(|r| {
                let remote_id = r.remote_id();
                (remote_id != &self.local_id).then(|| (remote_id, r))
            })
            .fold(BTreeMap::new(), |mut acc, (remote_id, r)| {
                acc.entry(remote_id).or_insert_with(Vec::new).push(r);
                acc
            });

        let mut updates = internal::Updates {
            tips: Vec::with_capacity(refs.len()),
            track: Vec::new(),
        };

        for (remote_id, refs) in grouped {
            let is_delegate = self.delegates.contains(remote_id);

            let mut tips_inner = Vec::with_capacity(refs.len());
            let mut track_inner = Vec::new();
            for r in refs {
                match &r.parsed.inner {
                    Left(refs::parsed::Rad::Me) => {
                        match Identities::verify(cx, r.tip, |_| None::<ObjectId>) {
                            Err(e) if is_delegate => {
                                return Err(error::Prepare::Verification(e.into()))
                            },
                            Err(e) => {
                                warn!(
                                    err = %e,
                                    remote_id = %remote_id,
                                    "skipping invalid `rad/self`"
                                );
                                continue;
                            },
                            Ok(id) => {
                                tips_inner.push(prepare_rad_self(cx, &id, r)?);
                                track_inner.push(track::Rel::SelfRef(id.urn()));
                            },
                        }
                    },

                    Left(refs::parsed::Rad::Id) => {
                        match Identities::verify(cx, r.tip, s.lookup_delegations(remote_id)) {
                            Err(e) if is_delegate => {
                                return Err(error::Prepare::Verification(e.into()))
                            },
                            Err(e) => {
                                warn!(
                                    err = %e,
                                    remote_id = %remote_id,
                                    "error verifying non-delegate id"
                                );
                                // Verification error for a non-delegate taints
                                // all refs for this remote_id
                                tips_inner.clear();
                                track_inner.clear();
                                break;
                            },

                            Ok(_) => {
                                if let Some(u) = mk_ref_update::<_, C::Urn>(r) {
                                    tips_inner.push(u)
                                }
                            },
                        }
                    },

                    Left(_) => {
                        if let Some(u) = mk_ref_update::<_, C::Urn>(r) {
                            tips_inner.push(u)
                        }
                    },

                    Right(_) => continue,
                }
            }

            updates.tips.append(&mut tips_inner);
            updates.track.append(&mut track_inner);
        }

        Ok(updates)
    }
}

/// If a top-level namespace exists for `id`, symref to it. Otherwise, create a
/// direct ref.
fn prepare_rad_self<'a, C, A>(
    cx: &C,
    id: &C::VerifiedIdentity,
    fr: &'a FilteredRef<A>,
) -> Result<Update<'a>, error::Prepare>
where
    C: Identities,
{
    use ids::{AnyIdentity as _, VerifiedIdentity as _};
    use refdb::{Policy, SymrefTarget};

    let urn = id.urn();
    let top = refs::namespaced(&id.urn(), refs::REFS_RAD_ID);
    let oid = Identities::get(cx, &urn)
        .map_err(|source| error::Prepare::FindRef {
            name: top.clone().into_qualified().into_refstring(),
            source: source.into(),
        })?
        .map(|id| id.content_id());

    let name = Qualified::from(fr.to_remote_tracking());
    let up = match oid {
        Some(oid) => Update::Symbolic {
            name,
            target: SymrefTarget {
                name: top,
                target: oid.as_ref().to_owned(),
            },
            type_change: Policy::Allow,
        },
        None => Update::Direct {
            name,
            target: fr.tip,
            no_ff: Policy::Abort,
        },
    };

    Ok(up)
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
