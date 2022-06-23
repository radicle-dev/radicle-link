// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use thiserror::Error;

use librad::{
    git::{identities, storage::Storage, tracking, Urn},
    paths::Paths,
    PeerId,
};
use std_ext::result::ResultExt as _;

use crate::git::include;

#[derive(Debug, Error)]
#[allow(clippy::large_enum_variant)]
pub enum Error {
    #[error(transparent)]
    Include(#[from] include::Error),

    #[error(transparent)]
    Track(#[from] tracking::error::Track),

    #[error(transparent)]
    Untrack(#[from] tracking::error::Untrack),
}

/// Track the given `urn` and `peer`. This will call
/// [`include::update`] for modifying the include file for the given
/// identity. If the `urn` does not exist in the storage, then no
/// action will be take for the include file.
///
/// See [`tracking::track`] for more on the semantics of tracking.
pub fn track(
    storage: &Storage,
    paths: &Paths,
    urn: &Urn,
    peer: Option<PeerId>,
    config: Option<tracking::Config>,
    policy: tracking::policy::Track,
) -> Result<Result<tracking::Ref, tracking::PreviousError>, Error> {
    let tracked = tracking::track(storage, urn, peer, config.unwrap_or_default(), policy)?;
    include::update(storage, paths, urn)
        .map(|path| tracing::info!(?path, "updated include file"))
        .or_matches::<Error, _, _>(is_not_found, || {
            tracing::warn!(%urn, "could not update include file, the URN did not exist");
            Ok(())
        })?;
    Ok(tracked)
}

/// Track the given `urn` and `peer`. This will call
/// [`include::update`] for modifying the include file for the given
/// identity. If the `urn` does not exist in the storage, then no
/// action will be take for the include file.
///
/// See [`tracking::untrack`] for more on the semantics of untracking.
pub fn untrack(
    storage: &Storage,
    paths: &Paths,
    urn: &Urn,
    peer: PeerId,
    args: tracking::UntrackArgs,
) -> Result<Result<tracking::Untracked<String>, tracking::PreviousError>, Error> {
    let untracked = tracking::untrack(storage, urn, peer, args)?;
    include::update(storage, paths, urn)
        .map(|path| tracing::info!(?path, "updated include file"))
        .or_matches::<Error, _, _>(is_not_found, || {
            tracing::warn!(%urn, "could not update include file, the URN did not exist");
            Ok(())
        })?;
    Ok(untracked)
}

fn is_not_found(err: &include::Error) -> bool {
    matches!(
        err,
        include::Error::Identities(identities::error::Error::NotFound(_))
            | include::Error::Relations(identities::relations::Error::Identities(
                identities::error::Error::NotFound(_),
            ))
    )
}
