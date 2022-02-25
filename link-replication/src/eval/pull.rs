// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::BTreeSet, fmt::Debug, marker::PhantomData};

use super::rad;
use crate::{
    error,
    fetch,
    ids,
    peek,
    refs,
    sigrefs,
    state::FetchState,
    validate,
    Error,
    FetchLimit,
    Identities,
    LocalIdentity,
    LocalPeer,
    Net,
    PeerId,
    Refdb,
    SignedRefs,
    SkippedFetch,
    Success,
    SymrefTarget,
    Tracking,
    Update,
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
        + SignedRefs<Oid = <C as Identities>::Oid>
        + Tracking<Urn = U>,
    <C as Identities>::Oid: Debug + PartialEq + Send + Sync + 'static,
{
    use either::Either::*;

    info!("fetching verification refs");
    let (
        peek::ForFetch {
            local_id,
            remote_id,
            delegates,
            mut tracked,
            limit: _,
        },
        skip,
    ) = {
        let spec = peek::for_fetch(&state.as_shim(cx), limit.peek, &anchor, remote_id)?;
        debug!(?spec);
        state.step(cx, spec)?
    };

    if matches!(skip, Some(SkippedFetch::NoMatchingRefs)) {
        return Ok(Success {
            applied: Default::default(),
            tracked: vec![],
            requires_confirmation: false,
            validation: vec![],
            _marker: PhantomData,
        });
    }

    let delegates: BTreeSet<PeerId> = delegates
        .into_iter()
        .filter(move |id| id != &local_id)
        .collect();

    let requires_confirmation = {
        info!("setting up local rad/ hierarchy");
        let shim = state.as_shim(cx);
        match ids::newest(&shim, &delegates)? {
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
    tracked.extend(newly_tracked.iter().filter_map(|x| x.as_ref().left()));

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

    info!("loading combined sigrefs");
    let signed_refs = sigrefs::combined(
        &state.as_shim(cx),
        sigrefs::Select {
            must: &delegates,
            may: &tracked,
            cutoff: 2,
        },
    )?;

    // Clear sigref tips so far. Fetch will ask the remote to advertise sigrefs
    // from the transitive trackings, so we can inspect the state afterwards to
    // see if we got any.
    state.sigref_tips_mut().clear();

    let step = fetch::Fetch {
        local_id,
        remote_id,
        signed_refs,
        limit: limit.data,
    };
    info!(?step, "fetching data");
    let (fetch::Fetch { signed_refs, .. }, _) = state.step(cx, step)?;

    if !state.sigref_tips().is_empty() {
        info!("transitively tracked signed refs found");
        let selector = sigrefs::Select {
            must: &Default::default(),
            // Optional, alt folks may have screwed their remotes
            may: &state.sigref_tips().keys().copied().collect(),
            cutoff: 0,
        };
        let signed_refs = sigrefs::combined(&state.as_shim(cx), selector)?;
        let step = fetch::Fetch {
            local_id,
            remote_id,
            signed_refs,
            limit: limit.data,
        };
        info!(?step, "fetching transitively tracked data");
        state.step(cx, step)?;
    }

    info!("post-validation");
    let warnings = validate(&state.as_shim(cx), &signed_refs)?;

    info!("updating tips");
    applied.append(&mut Refdb::update(cx, state.updates_mut().drain(..))?);
    for u in &applied.updated {
        debug!("applied {:?}", u);
    }

    info!("updating signed refs");
    SignedRefs::update(cx)?;

    Ok(Success {
        applied,
        tracked: newly_tracked,
        requires_confirmation,
        validation: warnings,
        _marker: PhantomData,
    })
}
