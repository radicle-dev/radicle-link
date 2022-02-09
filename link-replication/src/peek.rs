// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::collections::BTreeSet;

use itertools::Itertools as _;

use crate::{
    error,
    ids,
    refdb,
    refs,
    FilteredRef,
    Identities,
    LocalPeer,
    PeerId,
    RefPrefix,
    SignedRefs,
    Tracking,
    Update,
    VerifiedIdentity as _,
};

mod clone;
pub use clone::ForClone;

mod fetch;
pub use fetch::ForFetch;

pub fn for_fetch<C>(
    cx: &C,
    limit: u64,
    anchor: &C::VerifiedIdentity,
    remote_id: PeerId,
) -> Result<ForFetch, error::Error>
where
    C: Identities + LocalPeer + SignedRefs + Tracking<Urn = <C as Identities>::Urn>,
{
    let local_id = *LocalPeer::id(cx);
    let delegates = anchor.delegate_ids();
    let tracked = {
        let mut tracked = Tracking::tracked(cx)?.collect::<Result<BTreeSet<_>, _>>()?;
        let mut transitive = delegates
            .iter()
            .map(|did| SignedRefs::load(cx, did, 3))
            .filter_map_ok(|x| x.map(|y| y.remotes))
            .fold_ok(BTreeSet::new(), |mut acc, mut remotes| {
                acc.append(&mut remotes);
                acc
            })?;

        tracked.append(&mut transitive);
        tracked
            .into_iter()
            .filter(|id| !(delegates.contains(id) || id == &local_id))
            .collect::<BTreeSet<_>>()
    };

    Ok(ForFetch {
        local_id,
        remote_id,
        delegates: delegates.into_inner(),
        tracked,
        limit,
    })
}

fn ref_prefixes(id: &PeerId, remote_id: &PeerId) -> impl Iterator<Item = RefPrefix> {
    vec![
        refs::scoped(id, remote_id, refs::Owned::refs_rad_id()).into(),
        refs::scoped(id, remote_id, refs::Owned::refs_rad_self()).into(),
        refs::scoped(id, remote_id, refs::Owned::refs_rad_signed_refs()).into(),
        {
            let scope = (id != remote_id).then(|| id);
            RefPrefix::from_prefix(scope, refs::Prefix::RadIds)
        },
    ]
    .into_iter()
}

fn required_refs<'a>(
    id: &'a PeerId,
    remote_id: &PeerId,
) -> impl Iterator<Item = refs::Scoped<'a, 'static>> {
    vec![
        refs::scoped(id, remote_id, refs::Owned::refs_rad_id()),
        refs::scoped(id, remote_id, refs::Owned::refs_rad_signed_refs()),
    ]
    .into_iter()
}

fn guard_required<'a, 'b, 'c>(
    required_refs: BTreeSet<refs::Scoped<'a, 'b>>,
    wanted_refs: BTreeSet<refs::Scoped<'a, 'c>>,
) -> Result<(), error::Layout> {
    // We wanted nothing, so we can't expect anything
    if wanted_refs.is_empty() {
        return Ok(());
    }

    let diff = required_refs
        .difference(&wanted_refs)
        .map(|scoped| scoped.as_ref().to_owned())
        .collect::<Vec<_>>();

    if !diff.is_empty() {
        Err(error::Layout::MissingRequiredRefs(diff))
    } else {
        Ok(())
    }
}

fn mk_ref_update<T, Urn>(fref: &FilteredRef<T>) -> Option<Update<'_>>
where
    Urn: ids::Urn,
{
    use refdb::{Policy, SymrefTarget};
    use refs::parsed::Rad;

    let track_as = fref.to_remote_tracking();
    fref.parsed.inner.as_ref().left().and_then(|rad| match rad {
        Rad::Id | Rad::SignedRefs => Some(Update::Direct {
            name: track_as.into(),
            target: fref.tip,
            no_ff: Policy::Abort,
        }),

        Rad::Ids { urn } => Some(Update::Symbolic {
            name: track_as.into(),
            target: SymrefTarget {
                name: refs::namespaced(urn, refs::REFS_RAD_ID),
                target: fref.tip,
            },
            type_change: Policy::Allow,
        }),

        Rad::Me => None,
    })
}
