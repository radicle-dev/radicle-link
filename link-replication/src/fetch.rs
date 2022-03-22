// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::collections::HashSet;

use bstr::ByteSlice as _;
use git_ref_format::{name, refname, Component, Qualified};
use link_crypto::PeerId;
use link_git::protocol::{oid, Ref};
use radicle_data::NonEmptyVec;

use crate::{
    error,
    internal::{self, Layout, UpdateTips},
    refdb,
    refs,
    sigrefs,
    transmit::{self, BuildWantsHaves, LsRefs},
    FetchState,
    FilteredRef,
    Negotiation,
    Odb,
    Policy,
    RefPrefix,
    RefScan,
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
        NonEmptyVec::from_vec(prefixes.collect()).map(LsRefs::from)
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

    fn wants_haves<'a, R>(
        &self,
        db: &R,
        refs: &[FilteredRef<Self>],
    ) -> Result<Option<WantsHaves>, transmit::error::WantsHaves<R::FindError>>
    where
        R: Refdb + Odb,
    {
        let mut bld = BuildWantsHaves::default();

        for (remote_id, refs) in &self.signed_refs.refs {
            for (name, tip) in refs {
                // TODO: ensure sigrefs are well-formed. Or else, prune `refs`
                // iff `remote_id` is in delegates.
                let tracking = Qualified::from_refstr(name)
                    .and_then(|q| refs::remote_tracking(remote_id, q))
                    .ok_or_else(|| transmit::error::WantsHaves::Malformed(name.to_owned()))?;

                let want = match db.refname_to_id(tracking)? {
                    Some(oid) => {
                        let want = tip.as_ref() != oid.as_ref() && !db.contains(tip);
                        bld.have(oid.into());
                        want
                    },
                    None => !db.contains(tip),
                };
                if want {
                    bld.want(tip.as_ref().to_owned());
                }
            }
        }

        bld.add(db, refs)?;
        Ok(bld.build())
    }

    fn fetch_limit(&self) -> u64 {
        self.limit
    }
}

impl<T: AsRef<oid>> UpdateTips for Fetch<T> {
    fn prepare<'a, U, C>(
        &self,
        _: &FetchState<U>,
        cx: &C,
        _: &'a [FilteredRef<Self>],
    ) -> Result<internal::Updates<'a, U>, error::Prepare>
    where
        for<'b> &'b C: RefScan,
    {
        let mut tips = {
            let sz = self.signed_refs.refs.values().map(|rs| rs.refs.len()).sum();
            Vec::with_capacity(sz)
        };
        for (remote_id, refs) in &self.signed_refs.refs {
            let mut signed = HashSet::with_capacity(refs.refs.len());
            for (name, tip) in refs {
                let tracking: Qualified = Qualified::from_refstr(name)
                    .and_then(|q| refs::remote_tracking(remote_id, q.into_owned()))
                    .expect("we checked sigrefs well-formedness in wants_refs already")
                    .into();
                signed.insert(tracking.clone());
                tips.push(Update::Direct {
                    name: tracking,
                    target: tip.as_ref().to_owned(),
                    no_ff: Policy::Allow,
                });
            }

            // Prune refs not in signed
            let prefix = refname!("refs/remotes").join(Component::from(remote_id));
            let prefix_rad = prefix.join(name::RAD);
            let scan_err = |e: <&C as RefScan>::Error| error::Prepare::Scan { source: e.into() };
            for known in RefScan::scan(cx, prefix.as_str()).map_err(scan_err)? {
                let refdb::Ref { name, target, .. } = known.map_err(scan_err)?;
                // 'rad/' refs are never subject to pruning
                if name.starts_with(prefix_rad.as_str()) {
                    continue;
                }

                if !signed.contains(&name) {
                    tips.push(Update::Prune {
                        name,
                        prev: target.map_left(|oid| oid.into()),
                    });
                }
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
