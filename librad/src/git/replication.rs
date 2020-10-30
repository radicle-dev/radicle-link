// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use std::{
    collections::{BTreeMap, BTreeSet},
    convert::TryFrom,
    net::SocketAddr,
};

use git_ext::{self as ext, is_exists_err};
use std_ext::result::ResultExt as _;
use thiserror::Error;

use super::{
    fetch::{self, CanFetch as _},
    identities::{self, local::LocalIdentity},
    refs::{self, Refs},
    storage2::{self, Storage},
    tracking,
    types::{
        namespace::Namespace,
        reference::{self, Reference},
        Force,
    },
};
use crate::{
    identities::git::{Project, SomeIdentity, User},
    peer::PeerId,
    signer::Signer,
};

pub use crate::identities::git::Urn;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("identity not found")]
    MissingIdentity,

    #[error("missing required ref: {0}")]
    Missing(ext::RefLike),

    #[error("failed to convert {urn} to reference")]
    RefFromUrn {
        urn: Urn,
        source: reference::FromUrnError,
    },

    #[error(transparent)]
    Refs(#[from] refs::stored::Error),

    #[error(transparent)]
    Track(#[from] tracking::Error),

    #[error("signer error")]
    Sign(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),

    #[error("fetcher error")]
    Fetch(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),

    #[error(transparent)]
    Identities(#[from] identities::error::Error),

    #[error(transparent)]
    Store(#[from] storage2::Error),
}

/// Attempt to fetch `urn` from `remote_peer`, optionally supplying
/// `addr_hints`. `urn` may or may not already exist locally.
///
/// The [`Urn::path`] is ignored (defaulting to `rad/id`).
///
/// The fetch proceeds in three stages:
///
/// 1. Update the remote branches needed for identity verification
///
///    The identity according to `remote_peer` is verified, and if that passes
///    the local branch layout created (if it does not already exist). It may
///    also update local tracking relationships based on the identity
///    information.
///
/// 2. Fetch the `rad/signed_refs` of all tracked peers, and compute the
/// eligible heads (i.e. where    the `remote_peer` advertises the same tip oid
/// as found in the signed refs)
///
/// 3. Fetch the rest (i.e. eligible heads)
///
/// Optionally, a [`LocalIdentity`] can be specified to identify as in the
/// context of this namespace (ie. to be used as the `rad/self` branch). If not
/// specified, the existing identity is left untouched. If there is no existing
/// `rad/self` identity (eg. because this is the first time `urn` is fetched),
/// not specifying `whoami` is also referred to as "anonymous replication".
///
/// Note, however, that pushing local modifications requires a `rad/self` to be
/// set, which is enforced by the
/// [`crate::git::local::transport::LocalTransport`].
#[allow(clippy::unit_arg)]
#[tracing::instrument(skip(storage, whoami, addr_hints), err)]
pub fn replicate<S, Addrs>(
    storage: &Storage<S>,
    whoami: Option<LocalIdentity>,
    urn: Urn,
    remote_peer: PeerId,
    addr_hints: Addrs,
) -> Result<(), Error>
where
    S: Signer,
    Addrs: IntoIterator<Item = SocketAddr>,
{
    let urn = Urn::new(urn.id);
    let mut fetcher = storage.fetcher(urn.clone(), remote_peer, addr_hints)?;

    // Update identity branches first
    let _ = fetcher
        .fetch(fetch::Fetchspecs::Peek)
        .map_err(|e| Error::Fetch(e.into()))?;

    let delegates = match identities::any::get(storage, &urn)? {
        None => Err(Error::MissingIdentity),
        Some(some_id) => match some_id {
            SomeIdentity::User(user) => {
                ensure_setup_as_user(storage, user)?;
                Ok(None)
            },
            SomeIdentity::Project(proj) => {
                let delegates = ensure_setup_as_project(storage, proj)?;
                Ok(Some(delegates.collect()))
            },
        },
    }?;

    let tracked = tracking::tracked(storage, &urn)?.collect::<BTreeSet<_>>();
    // Fetch `signed_refs` for every peer we track now
    fetcher
        .fetch(fetch::Fetchspecs::SignedRefs {
            tracked: tracked.clone(),
        })
        .map_err(|e| Error::Fetch(e.into()))?;
    // Read `signed_refs` for all tracked
    let tracked_sigrefs = tracked
        .into_iter()
        .filter_map(|peer| match Refs::load(storage, &urn, peer) {
            Ok(Some(refs)) => Some(Ok((peer, refs))),

            Ok(None) => None,
            Err(e) => Some(Err(e)),
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    // Fetch all the rest
    fetcher
        .fetch(fetch::Fetchspecs::Replicate {
            tracked_sigrefs,
            delegates: delegates.unwrap_or_default(),
        })
        .map_err(|e| Error::Fetch(e.into()))?;

    // Update our signed refs
    Refs::update(storage, &urn)?;
    // Symref `rad/self` if a `LocalIdentity` was given
    if let Some(local_id) = whoami {
        local_id.link(storage, &urn)?;
    }

    // TODO: At this point, the tracking graph may have changed, and/or we
    // created top-level user namespaces. We will eventually converge, but
    // perhaps we'd want to return some kind of continuation here, so the caller
    // could schedule a deferred task directly?

    Ok(())
}

fn ensure_setup_as_user<S>(storage: &Storage<S>, user: User) -> Result<(), Error>
where
    S: Signer,
{
    let urn = user.urn();
    match identities::user::verify(storage, &urn)? {
        None => Err(Error::MissingIdentity),
        Some(user) => {
            // Create `rad/id` here, if not exists
            ensure_rad_id(storage, &urn, user.content_id)?;

            // Track all delegations
            for key in user.into_inner().doc.delegations {
                tracking::track(storage, &urn, PeerId::from(key))?;
            }

            Ok(())
        },
    }
}

fn ensure_setup_as_project<S>(
    storage: &Storage<S>,
    proj: Project,
) -> Result<impl Iterator<Item = Urn>, Error>
where
    S: Signer,
{
    let urn = proj.urn();

    // Verify + symref the delegates first
    for delegate in proj.doc.delegations.iter().indirect() {
        let delegate_urn = delegate.urn();
        // Find in `refs/namespaces/<urn>/rad/ids/<delegate.urn>`
        let in_rad_ids = Urn {
            path: Some(reflike!("rad/ids").join(&delegate_urn)),
            ..urn.clone()
        };

        match identities::user::verify(storage, &in_rad_ids)? {
            None => Err(Error::Missing(in_rad_ids.into())),
            Some(delegate_user) => {
                // Ensure we have a top-level `refs/namespaces/<delegate>/rad/id`
                //
                // Either we fetched that before, or we take `remote_peer`s view
                // (we just verified the identity).
                ensure_rad_id(storage, &delegate_urn, delegate_user.content_id)?;
                // Also, track them
                for key in delegate_user.doc.delegations.iter() {
                    tracking::track(storage, &delegate_urn, PeerId::from(*key))?;
                }
                // Now point our view to the top-level
                Reference::try_from(&delegate_urn)
                    .map_err(|e| Error::RefFromUrn {
                        urn: delegate_urn.clone(),
                        source: e,
                    })?
                    .symbolic_ref::<_, PeerId>(
                        Reference::rad_delegate(Namespace::from(&urn), &delegate_urn),
                        Force::False,
                    )
                    .create(storage.as_raw())
                    .and(Ok(()))
                    .or_matches(is_exists_err, || Ok(()))
                    .map_err(|e: git2::Error| Error::Store(e.into()))
            },
        }?;
    }

    let proj = identities::project::verify(storage, &urn)?
        .ok_or(Error::MissingIdentity)?
        .into_inner();

    // Create `rad/id` here, if not exists
    ensure_rad_id(storage, &urn, proj.content_id)?;

    // Make sure we track any direct delegations
    for key in proj.doc.delegations.iter().direct() {
        tracking::track(storage, &urn, PeerId::from(*key))?;
    }

    Ok(proj
        .doc
        .delegations
        .into_iter()
        .indirect()
        .map(|id| id.urn()))
}

fn ensure_rad_id<S>(storage: &Storage<S>, urn: &Urn, tip: ext::Oid) -> Result<(), Error>
where
    S: Signer,
{
    identities::common::IdRef::from(urn)
        .create(storage, tip)
        .map_err(|e| Error::Store(e.into()))
}
