// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::collections::BTreeSet;

use crate::{error, refs, PeerId, RefPrefix};

mod clone;
pub use clone::ForClone;

mod fetch;
pub use fetch::{ForFetch, Spec as FetchSpec};

pub(crate) fn ref_prefixes(id: &PeerId, remote_id: &PeerId) -> impl Iterator<Item = RefPrefix> {
    IntoIterator::into_iter([
        refs::scoped(id, remote_id, refs::Owned::refs_rad_id()).into(),
        refs::scoped(id, remote_id, refs::Owned::refs_rad_self()).into(),
        refs::scoped(id, remote_id, refs::Owned::refs_rad_signed_refs()).into(),
        {
            let scope = (id != remote_id).then_some(id);
            RefPrefix::from_prefix(scope, refs::Prefix::RadIds)
        },
    ])
}

pub(crate) fn required_refs<'a>(
    id: &'a PeerId,
    remote_id: &PeerId,
) -> impl Iterator<Item = refs::Scoped<'a, 'static>> {
    IntoIterator::into_iter([
        refs::scoped(id, remote_id, refs::Owned::refs_rad_id()),
        refs::scoped(id, remote_id, refs::Owned::refs_rad_signed_refs()),
    ])
}

pub(crate) fn guard_required<'a, 'b, 'c>(
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
