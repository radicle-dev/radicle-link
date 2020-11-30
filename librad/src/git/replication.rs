// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::{BTreeMap, BTreeSet},
    convert::{TryFrom, TryInto},
    net::SocketAddr,
};

use git_ext::{self as ext, is_exists_err};
use std_ext::result::ResultExt as _;
use thiserror::Error;

use super::{
    fetch::{self, CanFetch as _},
    identities::{self, local::LocalIdentity},
    refs::{self, Refs},
    storage::{self, Storage},
    tracking,
    types::{reference, Force, Namespace, Reference},
};
use crate::{
    identities::git::{Person, Project, SomeIdentity},
    peer::PeerId,
};

pub use crate::identities::git::Urn;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("cannot replicate from self")]
    SelfReplication,

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
    Store(#[from] storage::Error),
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
///    eligible heads (i.e. where the `remote_peer` advertises the same tip
///    oid as found in the signed refs)
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
pub fn replicate<Addrs>(
    storage: &Storage,
    whoami: Option<LocalIdentity>,
    urn: Urn,
    remote_peer: PeerId,
    addr_hints: Addrs,
) -> Result<(), Error>
where
    Addrs: IntoIterator<Item = SocketAddr>,
{
    if storage.peer_id() == &remote_peer {
        return Err(Error::SelfReplication);
    }

    let urn = Urn::new(urn.id);
    let mut fetcher = storage.fetcher(urn.clone(), remote_peer, addr_hints)?;

    // Update identity branches first
    tracing::debug!("updating identity branches");
    let _ = fetcher
        .fetch(fetch::Fetchspecs::Peek)
        .map_err(|e| Error::Fetch(e.into()))?;

    let remote_ident: Urn = Reference::rad_id(Namespace::from(&urn))
        .with_remote(remote_peer)
        .try_into()
        .expect("namespace is set");
    let delegates = match identities::any::get(storage, &remote_ident)? {
        None => Err(Error::MissingIdentity),
        Some(some_id) => match some_id {
            SomeIdentity::Person(person) => {
                ensure_setup_as_person(storage, person, remote_peer)?;
                Ok(None)
            },
            SomeIdentity::Project(proj) => {
                let delegates = ensure_setup_as_project(storage, proj, remote_peer)?;
                Ok(Some(delegates.collect()))
            },
        },
    }?;

    let tracked = tracking::tracked(storage, &urn)?.collect::<BTreeSet<_>>();
    // Fetch `signed_refs` for every peer we track now
    tracing::debug!("fetching signed refs");
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
    tracing::debug!("fetching heads: {:?}, {:?}", tracked_sigrefs, delegates);
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
    // created top-level person namespaces. We will eventually converge, but
    // perhaps we'd want to return some kind of continuation here, so the caller
    // could schedule a deferred task directly?

    Ok(())
}

#[allow(clippy::unit_arg)]
#[tracing::instrument(level = "trace", skip(storage), err)]
fn ensure_setup_as_person(
    storage: &Storage,
    person: Person,
    remote_peer: PeerId,
) -> Result<(), Error> {
    let urn: Urn = Reference::rad_id(Namespace::from(person.urn()))
        .with_remote(remote_peer)
        .try_into()
        .expect("namespace is set");

    match identities::person::verify(storage, &urn)? {
        None => Err(Error::MissingIdentity),
        Some(person) => {
            // Create `rad/id` here, if not exists
            ensure_rad_id(storage, &urn, person.content_id)?;

            // Track all delegations
            for key in person.into_inner().doc.delegations {
                tracking::track(storage, &urn, PeerId::from(key))?;
            }

            Ok(())
        },
    }
}

#[tracing::instrument(level = "trace", skip(storage), err)]
fn ensure_setup_as_project(
    storage: &Storage,
    proj: Project,
    remote_peer: PeerId,
) -> Result<impl Iterator<Item = Urn>, Error> {
    let local_peer_id = storage.peer_id();

    let urn: Urn = Reference::rad_id(Namespace::from(proj.urn()))
        .with_remote(remote_peer)
        .try_into()
        .expect("namespace is set");

    // Verify + symref the delegates first
    for delegate in proj.delegations().iter().indirect() {
        let delegate_urn = delegate.urn();
        // Find in `refs/namespaces/<urn>/refs/remotes/<remote
        // peer>/rad/ids/<delegate.urn>`
        let in_rad_ids: Urn = Reference::rad_delegate(Namespace::from(&urn), &delegate_urn)
            .with_remote(remote_peer)
            .try_into()
            .expect("namespace is set");
        match identities::person::verify(storage, &in_rad_ids)? {
            None => Err(Error::Missing(in_rad_ids.into())),
            Some(delegate_person) => {
                // Ensure we have a top-level `refs/namespaces/<delegate>/rad/id`
                //
                // Either we fetched that before, or we take `remote_peer`s view
                // (we just verified the identity).
                ensure_rad_id(storage, &delegate_urn, delegate_person.content_id)?;
                // Also, track them
                for key in delegate_person.delegations().iter() {
                    let peer_id = PeerId::from(*key);
                    if &peer_id != local_peer_id {
                        // Top-level
                        tracking::track(storage, &delegate_urn, peer_id)?;
                        // as well as for `proj`
                        tracking::track(storage, &proj.urn(), peer_id)?;
                    }
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
    for key in proj
        .delegations()
        .iter()
        .direct()
        .filter(|&key| key != local_peer_id.as_public_key())
    {
        tracking::track(storage, &urn, PeerId::from(*key))?;
    }

    Ok(proj
        .doc
        .delegations
        .into_iter()
        .indirect()
        .map(|id| id.urn()))
}

#[allow(clippy::unit_arg)]
#[tracing::instrument(level = "trace", skip(storage), err)]
fn ensure_rad_id(storage: &Storage, urn: &Urn, tip: ext::Oid) -> Result<(), Error> {
    identities::common::IdRef::from(urn)
        .create(storage, tip)
        .map_err(|e| Error::Store(e.into()))
}
