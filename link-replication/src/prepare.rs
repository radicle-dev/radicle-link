// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeMap;

use git_ref_format::Qualified;
use link_crypto::PeerId;
use link_git::protocol::ObjectId;

use crate::{
    error,
    ids,
    internal,
    refdb,
    refs,
    state::FetchState,
    track,
    FilteredRef,
    Identities,
    RefScan,
    Update,
};

pub(crate) fn verification_refs<'a, U, C, T, F>(
    local_id: &PeerId,
    s: &FetchState<U>,
    cx: &C,
    refs: &'a [FilteredRef<T>],
    is_delegate: F,
) -> Result<internal::Updates<'a, U>, error::Prepare>
where
    U: ids::Urn + Ord,
    C: Identities<Urn = U>,
    for<'b> &'b C: RefScan,
    F: Fn(&PeerId) -> bool,
{
    use either::Either::*;
    use ids::VerifiedIdentity as _;

    let grouped: BTreeMap<&PeerId, Vec<&FilteredRef<T>>> = refs
        .iter()
        .filter_map(|r| {
            let remote_id = r.remote_id();
            (remote_id != local_id).then_some((remote_id, r))
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
        let is_delegate = is_delegate(remote_id);

        let mut tips_inner = Vec::with_capacity(refs.len());
        let mut track_inner = Vec::new();
        for r in refs {
            match &r.parsed.inner {
                Left(refs::parsed::Rad::Selv) => {
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
                            tips_inner.push(rad_self(cx, &id, r)?);
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
                            if let Some(u) = r.as_verification_ref_update() {
                                tips_inner.push(u)
                            }
                        },
                    }
                },

                Left(_) => {
                    if let Some(u) = r.as_verification_ref_update() {
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

/// If a top-level namespace exists for `id`, symref to it. Otherwise, create a
/// direct ref.
pub(crate) fn rad_self<'a, C, A>(
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
