// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::BTreeMap, fmt::Debug, marker::PhantomData};

use itertools::Itertools;

use super::rad;
use crate::{
    error,
    fetch,
    ids,
    peek,
    refs,
    sigrefs::{self, Refs},
    state::FetchState,
    validation,
    DataPolicy,
    Error,
    FetchLimit,
    Identities,
    LocalIdentity,
    LocalPeer,
    Net,
    Odb,
    PeerId,
    RefScan,
    Refdb,
    SignedRefs,
    Sigrefs,
    Success,
    SymrefTarget,
    Tracking,
    Update,
    VerifiedIdentity,
};

pub(crate) fn pull<U, C>(
    state: &mut FetchState<U>,
    cx: &mut C,
    limit: FetchLimit,
    anchor: C::VerifiedIdentity,
    remote_id: PeerId,
    whoami: Option<LocalIdentity>,
) -> Result<Success<<C as Identities>::Urn>, Error>
where
    U: ids::Urn + Clone + Debug + Ord,
    C: Identities<Urn = U>
        + LocalPeer
        + Net
        + Refdb
        + Odb
        + SignedRefs<Oid = <C as Identities>::Oid>
        + Tracking<Urn = U>,
    <C as Identities>::Oid: Debug + PartialEq + Send + Sync + 'static,
    for<'a> &'a C: RefScan,
{
    use either::Either::*;

    let scx = state.as_shim(cx);
    let local_id = *LocalPeer::id(&scx);
    let delegates = VerifiedIdentity::delegate_ids(&anchor);
    let delegates_sans_local = delegates
        .iter()
        .filter(|id| *id != &local_id)
        .copied()
        .collect();

    let tracked: BTreeMap<PeerId, peek::FetchSpec> = Tracking::tracked(&scx)?
        .filter_map_ok(|(id, policy)| {
            if !delegates.contains(&id) {
                Some((
                    id,
                    peek::FetchSpec {
                        is_delegate: false,
                        policy,
                    },
                ))
            } else {
                None
            }
        })
        .chain(delegates.iter().map(|id| {
            Ok((
                *id,
                peek::FetchSpec {
                    is_delegate: true,
                    policy: DataPolicy::Allow,
                },
            ))
        }))
        .collect::<Result<_, _>>()?;

    info!("fetching verification refs");
    let peek = peek::ForFetch {
        local_id,
        remote_id,
        tracked,
        limit: limit.peek,
    };
    debug!(?peek);
    state.step(cx, &peek)?;

    info!("loading sigrefs");
    let signed_refs = sigrefs::combined(
        &state.as_shim(cx),
        sigrefs::Select {
            must: &delegates_sans_local,
            may: &peek
                .tracked
                .keys()
                .filter(|id| !delegates.contains(id))
                .copied()
                .collect(),
            cutoff: 2,
        },
    )?;
    debug!(?signed_refs);

    let mut transitive: BTreeMap<PeerId, DataPolicy> = BTreeMap::new();
    for (id, spec) in &peek.tracked {
        if let Some(sigrefs) = signed_refs.get(id) {
            for remote_id in &sigrefs.remotes {
                if remote_id == &local_id
                    || delegates.contains(remote_id)
                    || peek.tracked.contains_key(remote_id)
                {
                    continue;
                }
                transitive
                    .entry(*remote_id)
                    .and_modify(|v| {
                        if !spec.is_delegate && spec.policy < *v {
                            *v = spec.policy;
                        }
                    })
                    .or_insert(spec.policy);
            }
        }
    }

    let requires_confirmation = {
        info!("setting up local rad/ hierarchy");
        let shim = state.as_shim(cx);
        match ids::newest(&shim, &delegates_sans_local)? {
            None => false,
            Some((their_id, theirs)) => match rad::newer(&shim, Some(anchor), theirs)? {
                Err(error::ConfirmationRequired) => true,
                Ok(newest) => {
                    let rad::Rad { mut track, up } = match newest {
                        Left(ours) => rad::setup(&shim, None, &ours, whoami)?,
                        Right(theirs) => rad::setup(&shim, Some(their_id), &theirs, whoami)?,
                    };

                    state.trackings_mut().append(&mut track);
                    state.update_all(up);

                    false
                },
            },
        }
    };

    // Apply trackings disovered so far. If this fails, we haven't transferred a
    // lot of data yet.
    info!("updating trackings");
    let newly_tracked = Tracking::track(cx, state.trackings_mut().drain(..))?
        .into_iter()
        .collect::<Vec<_>>();

    // Update identity tips already, we will only be looking at sigrefs from now
    // on. Can improve concurrency.
    info!("updating identity tips");
    let mut applied = {
        let pending = state.updates_mut();

        // `Vec::drain_filter` would help here
        let mut tips = Vec::new();
        let mut i = 0;
        while i < pending.len() {
            match &pending[i] {
                Update::Direct { name, .. } if name.ends_with(refs::name::str::REFS_RAD_ID) => {
                    tips.push(pending.swap_remove(i));
                },
                Update::Symbolic {
                    target: SymrefTarget { name, .. },
                    ..
                } if name.ends_with(refs::name::str::REFS_RAD_ID) => {
                    tips.push(pending.swap_remove(i));
                },
                _ => {
                    i += 1;
                },
            }
        }
        Refdb::update(cx, tips)?
    };

    let signed_refs = signed_refs.flattened();
    // Clear rad tips so far. Fetch will ask the remote to advertise
    // all rad refs from the transitive trackings, so we can inspect
    // the state afterwards to see if we got any.
    state.clear_rad_refs();

    let fetch = fetch::Fetch {
        local_id,
        remote_id,
        signed_refs,
        limit: limit.data,
    };
    info!("fetching data");
    debug!(?fetch);
    state.step(cx, &fetch)?;

    let mut signed_refs = fetch.signed_refs;

    if !state.id_tips().is_empty() {
        info!("transitively tracked data found");
        let selector = sigrefs::Select {
            must: &Default::default(),
            may: &state
                .sigref_tips()
                .keys()
                .filter(|id| matches!(transitive.get(id), Some(DataPolicy::Allow)))
                .copied()
                .collect(),
            cutoff: 0,
        };
        let trans_sigrefs = sigrefs::combined(&state.as_shim(cx), selector)?;
        let trans_ids = state.id_tips().keys().copied().collect();
        debug!(?trans_sigrefs);
        let trans_fetch = fetch::Transitive {
            local_id,
            remote_id,
            signed_refs: trans_sigrefs,
            identities: trans_ids,
            denied: transitive
                .into_iter()
                .filter_map(|(id, policy)| matches!(policy, DataPolicy::Deny).then(|| id))
                .collect(),
            limit: limit.data,
        };
        info!("fetching transitively tracked data");
        debug!(?trans_fetch);
        state.step(cx, &trans_fetch)?;
        signed_refs
            .refs
            .append(&mut trans_fetch.signed_refs.flattened().refs);
    }

    info!("updating tips");
    applied.append(&mut Refdb::update(cx, state.updates_mut().drain(..))?);
    for u in &applied.updated {
        debug!("applied {:?}", u);
    }

    info!("updating signed refs");
    SignedRefs::update(cx)?;

    let mut warnings = Vec::new();
    debug!(?signed_refs);
    info!("validating signed trees");
    for (peer, refs) in &signed_refs.refs {
        let ws = validation::validate::<U, _, _, _>(&*cx, peer, refs)?;
        debug_assert!(
            ws.is_empty(),
            "expected no warnings for {}, but got {:?}",
            peer,
            ws
        );
        warnings.extend(ws);
    }

    info!("validating remote trees");
    for peer in &signed_refs.remotes {
        if peer == &local_id {
            continue;
        }
        debug!("remote {}", peer);
        let refs = SignedRefs::load(cx, peer, 0)
            .map(|s| s.map(|Sigrefs { at, refs, .. }| Refs { at, refs }))?;
        match refs {
            None => warnings.push(error::Validation::NoData((*peer).into())),
            Some(refs) => {
                let ws = validation::validate::<U, _, _, _>(&*cx, peer, &refs)?;
                debug_assert!(
                    ws.is_empty(),
                    "expected no warnings for remote {}, but got {:?}",
                    peer,
                    ws
                );
                warnings.extend(ws);
            },
        }
    }
    Ok(Success {
        applied,
        tracked: newly_tracked,
        requires_confirmation,
        validation: warnings,
        _marker: PhantomData,
    })
}
