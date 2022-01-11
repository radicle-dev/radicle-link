// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{borrow::Cow, collections::BTreeSet};

use bstr::{BString, ByteSlice as _};
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

fn ref_prefixes<'a>(
    id: &'a PeerId,
    remote_id: &PeerId,
) -> impl Iterator<Item = refs::Scoped<'a, 'static>> {
    vec![
        refs::scoped(id, remote_id, refs::RadId),
        refs::scoped(id, remote_id, refs::RadSelf),
        refs::scoped(id, remote_id, refs::Prefix::RadIds),
        refs::scoped(id, remote_id, refs::Signed),
    ]
    .into_iter()
}

fn required_refs<'a>(
    id: &'a PeerId,
    remote_id: &PeerId,
) -> impl Iterator<Item = refs::Scoped<'a, 'static>> {
    vec![
        refs::scoped(id, remote_id, refs::RadId),
        refs::scoped(id, remote_id, refs::Signed),
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
    use ids::Urn as _;
    use refdb::{Policy, SymrefTarget};
    use refs::parsed::Rad;

    let track_as = Cow::from(refs::remote_tracking(&fref.remote_id, fref.name.as_bstr()));
    fref.parsed.as_ref().left().and_then(|rad| match rad {
        Rad::Id | Rad::SignedRefs => Some(Update::Direct {
            name: track_as,
            target: fref.tip,
            no_ff: Policy::Abort,
        }),

        Rad::Ids { urn } => Some(Update::Symbolic {
            name: track_as,
            target: SymrefTarget {
                name: refs::Namespaced {
                    namespace: Some(BString::from(urn.encode_id()).into()),
                    refname: refs::RadId.into(),
                },
                target: fref.tip,
            },
            type_change: Policy::Allow,
        }),

        Rad::Me => None,
    })
}
