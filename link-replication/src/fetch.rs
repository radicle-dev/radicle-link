// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::collections::{BTreeSet, HashSet};

use bstr::ByteSlice as _;
use git_ref_format::Qualified;
use link_crypto::PeerId;
use link_git::protocol::{oid, Ref};
use radicle_data::NonEmptyVec;

use crate::{
    error,
    internal::{self, Layout, UpdateTips},
    refs,
    sigrefs,
    transmit::{self, ExpectLs, LsRefs},
    FetchState,
    FilteredRef,
    Identities,
    Negotiation,
    Policy,
    RefPrefix,
    Refdb,
    Update,
    WantsHaves,
};

#[derive(Debug)]
pub struct Fetch<Oid> {
    /// The local id.
    pub local_id: PeerId,
    /// The peer being fetched from.
    pub remote_id: PeerId,
    /// The stack of signed refs describing which refs we'll ask for.
    pub signed_refs: sigrefs::Combined<Oid>,
    /// Maximum number of bytes the fetched packfile can have.
    pub limit: u64,
}

impl<T: AsRef<oid>> Negotiation for Fetch<T> {
    fn ls_refs(&self) -> Option<LsRefs> {
        let prefixes = self.signed_refs.remotes.iter().filter_map(|id| {
            (id != &self.remote_id).then(|| {
                RefPrefix::from(refs::scoped(
                    id,
                    &self.remote_id,
                    refs::Owned::refs_rad_signed_refs(),
                ))
            })
        });
        NonEmptyVec::from_vec(prefixes.collect()).map(|prefixes| LsRefs::Prefix {
            prefixes,
            response: ExpectLs::MayEmpty,
        })
    }

    fn ref_filter(&self, r: Ref) -> Option<FilteredRef<Self>> {
        use either::Either::*;
        use refs::parsed::Identity;

        let (refname, tip) = refs::into_unpacked(r);
        let parsed = refs::parse::<Identity>(refname.as_bstr()).ok()?;
        match parsed {
            refs::Parsed {
                remote: Some(remote_id),
                inner: Left(refs::parsed::Rad::SignedRefs),
            } if self.signed_refs.remotes.contains(&remote_id) => {
                Some(FilteredRef::new(tip, &remote_id, parsed))
            },

            _ => None,
        }
    }

    fn wants_haves<'a, R: Refdb>(
        &self,
        db: &R,
        refs: impl IntoIterator<Item = FilteredRef<Self>>,
    ) -> Result<WantsHaves<Self>, transmit::error::WantsHaves<R::FindError>> {
        let mut wanted = HashSet::new();
        let mut wants = BTreeSet::new();
        let mut haves = BTreeSet::new();

        for (remote_id, refs) in &self.signed_refs.refs {
            for (name, tip) in refs {
                // TODO: ensure sigrefs are well-formed. Or else, prune `refs`
                // iff `remote_id` is in delegates.
                let tracking = Qualified::from_refstr(name)
                    .and_then(|q| refs::remote_tracking(remote_id, q))
                    .ok_or_else(|| transmit::error::WantsHaves::Malformed(name.to_owned()))?;

                if let Some(oid) = db.refname_to_id(tracking)? {
                    haves.insert(oid.as_ref().to_owned());
                    // TODO: do we want to check if `tip` is in the ancestry
                    // path? This could be a reset to a previous version.
                    if tip.as_ref() != oid.as_ref() {
                        wants.insert(tip.as_ref().to_owned());
                    }
                } else {
                    wants.insert(tip.as_ref().to_owned());
                }
            }
        }

        for r in refs {
            let have = db.refname_to_id(r.to_remote_tracking())?;
            if let Some(oid) = have.as_ref() {
                haves.insert(oid.as_ref().to_owned());
                // TODO: as above, should perform ancestry check?
                if r.tip.as_ref() != oid.as_ref() {
                    wants.insert(r.tip);
                    wanted.insert(r);
                }
            } else {
                wants.insert(r.tip);
                wanted.insert(r);
            }
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

impl<T: AsRef<oid>> UpdateTips for Fetch<T> {
    fn prepare<'a, U, C>(
        &self,
        _: &FetchState<U>,
        _: &C,
        _: &'a [FilteredRef<Self>],
    ) -> Result<internal::Updates<'a, U>, error::Prepare>
    where
        C: Identities,
    {
        let mut tips = {
            let sz = self.signed_refs.refs.values().map(|rs| rs.refs.len()).sum();
            Vec::with_capacity(sz)
        };
        for (remote_id, refs) in &self.signed_refs.refs {
            for (name, tip) in refs {
                let tracking = Qualified::from_refstr(name)
                    .and_then(|q| refs::remote_tracking(remote_id, q.into_owned()))
                    .expect("we checked sigrefs well-formedness in wants_refs already");
                tips.push(Update::Direct {
                    name: tracking.into(),
                    target: tip.as_ref().to_owned(),
                    no_ff: Policy::Allow,
                });
            }
        }

        Ok(internal::Updates {
            tips,
            track: vec![],
        })
    }
}

impl<T> Layout for Fetch<T> {
    // [`Fetch`] may request only a part of the refs tree, so no layout error
    // can be determined from the advertised refs alone.
    //
    // XXX: We could reject if only a subset of the signed refs are present. This
    // would interact with fetchspecs, so requires runtime configuration.
    fn pre_validate(&self, _: &[FilteredRef<Self>]) -> Result<(), error::Layout> {
        Ok(())
    }
}
