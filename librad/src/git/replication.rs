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
    identities,
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
    Verify(#[from] crate::identities::git::error::Verify),

    #[error(transparent)]
    Identities(#[from] identities::error::Error),

    #[error(transparent)]
    Store(#[from] storage2::Error),
}

pub fn replicate<S, Addrs>(
    storage: &Storage<S>,
    urn: Urn,
    remote_peer: PeerId,
    addr_hints: Addrs,
) -> Result<(), Error>
where
    S: Signer,
    S::Error: std::error::Error + Send + Sync + 'static,

    Addrs: IntoIterator<Item = SocketAddr>,
{
    let mut fetcher = storage.fetcher(urn.clone(), remote_peer, addr_hints)?;

    if storage.has_urn(&urn)? {
        let _ = fetcher
            .fetch(fetch::Fetchspecs::Peek)
            .map_err(|e| Error::Fetch(e.into()))?;
    }

    let delegates = match identities::any::get(storage, &urn)? {
        None => Err(Error::MissingIdentity),
        Some(some_id) => match some_id {
            SomeIdentity::User(user) => {
                ensure_setup_as_user(storage, &urn, user)?;
                Ok(None)
            },
            SomeIdentity::Project(proj) => {
                let delegates = ensure_setup_as_project(storage, &urn, proj)?;
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
    let tracked_sigrefs = tracked.iter().try_fold(BTreeMap::new(), |mut acc, peer| {
        if let Some(refs) = Refs::load(storage, &urn, *peer)? {
            acc.insert(*peer, refs);
        }

        Ok::<_, Error>(acc)
    })?;

    fetcher
        .fetch(fetch::Fetchspecs::Replicate {
            tracked_sigrefs,
            delegates: delegates.unwrap_or_default(),
        })
        .map_err(|e| Error::Fetch(e.into()))?;

    // Update our signed refs
    Refs::update(storage, &urn)?;

    // decide what should be fetched later

    Ok(())
}

fn ensure_setup_as_user<S>(storage: &Storage<S>, urn: &Urn, user: User) -> Result<(), Error>
where
    S: Signer,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let user = storage
        .identities::<'_, User>()
        .verify(*user.content_id)
        .map_err(|e| Error::Verify(e.into()))?
        .into_inner();

    // Create `rad/id` here, if not exists
    ensure_rad_id(storage, &urn, user.content_id)?;

    // Track all delegations
    for key in user.doc.delegations {
        tracking::track(storage, &urn, PeerId::from(key))?;
    }

    Ok(())
}

fn ensure_setup_as_project<S>(
    storage: &Storage<S>,
    urn: &Urn,
    proj: Project,
) -> Result<impl Iterator<Item = Urn>, Error>
where
    S: Signer,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    // Verify + symref the delegates first
    for delegate in proj.doc.delegations.iter().indirect() {
        let delegate_urn = delegate.urn();
        // Find in `refs/namespaces/<urn>/rad/ids/<delegate.urn>`
        let in_rad_ids = Urn {
            path: Some(reflike!("rad/ids").join(&delegate_urn)),
            ..urn.clone()
        };

        match identities::user::get(storage, &in_rad_ids)? {
            None => Err(Error::Missing(in_rad_ids.into())),
            Some(delegate_user) => {
                let delegate_user = storage
                    .identities::<'_, User>()
                    .verify(*delegate_user.content_id)
                    .map_err(|e| Error::Verify(e.into()))?
                    .into_inner();

                // Ensure we have a top-level `refs/namespaces/<delegate>/rad/id`
                //
                // Either we fetched that before, or we take `remote_peer`s view
                // (we just verified the identity).
                ensure_rad_id(storage, &delegate_urn, delegate_user.content_id)?;
                // Also, track them
                for key in delegate_user.doc.delegations {
                    tracking::track(storage, &delegate_urn, PeerId::from(key))?;
                }
                // Now point our view to the top-level
                Reference::try_from(&delegate_urn)
                    .map_err(|e| Error::RefFromUrn {
                        urn: delegate_urn.clone(),
                        source: e,
                    })?
                    .symbolic_ref::<_, PeerId>(
                        Reference::rad_delegate(Namespace::from(urn), &delegate_urn),
                        Force::False,
                    )
                    .create(storage.as_raw())
                    .and(Ok(()))
                    .or_matches(is_exists_err, || Ok(()))
                    .map_err(|e: git2::Error| Error::Store(e.into()))
            },
        }?;
    }

    let proj = storage
        .identities::<'_, Project>()
        .verify(*proj.content_id, |urn| find_latest_head(storage, urn))
        .map_err(|e| Error::Verify(e.into()))?
        .into_inner();

    // Create `rad/id` here, if not exists
    ensure_rad_id(storage, urn, proj.content_id)?;

    // Make sure we track any direct delegations
    for key in proj.doc.delegations.iter().direct() {
        tracking::track(storage, urn, PeerId::from(*key))?;
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
    S::Error: std::error::Error + Send + Sync + 'static,
{
    Reference::<_, PeerId, _>::rad_id(Namespace::from(urn))
        .create(
            storage.as_raw(),
            *tip,
            Force::False,
            &format!("Initial rad/id for {}: {}", urn, tip),
        )
        .and(Ok(()))
        .map_err(|e| Error::Store(e.into()))
}

#[derive(Debug, Error)]
pub enum LookupError {
    #[error("identity at {0} not available")]
    NotFound(Urn),

    #[error(transparent)]
    Store(#[from] storage2::Error),
}

fn find_latest_head<S>(storage: &Storage<S>, urn: Urn) -> Result<git2::Oid, LookupError>
where
    S: Signer,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    storage
        .tip(&urn)?
        .map(|oid| *oid)
        .ok_or_else(|| LookupError::NotFound(urn))
}
