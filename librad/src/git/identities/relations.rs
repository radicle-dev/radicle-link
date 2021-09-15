// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryFrom as _;

use either::Either;

use crate::{
    git::{
        identities,
        refs::{stored, Refs},
        storage::{self, ReadOnlyStorage as _},
        tracking,
        types::{Namespace, Reference},
        Urn,
    },
    identities::{
        relations::{Peer, Role, Status},
        Person,
        SomeIdentity,
    },
    PeerId,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Identities(#[from] identities::Error),
    #[error(transparent)]
    Storage(#[from] storage::Error),
    #[error(transparent)]
    Stored(#[from] stored::Error),
    #[error(transparent)]
    Tracking(#[from] tracking::Error),
    #[error("the identity `{0}` found is not recognised/supported")]
    UknownIdentity(Urn),
}

/// Determine the [`Role`] for [`SomeIdentity`] and [`PeerId`].
///
/// The rules for determining the role are:
///   * If the peer is one of the delegates they are considred a
///     [`Role::Maintainer`]
///   * If the peer has made changes and published `rad/signed_refs` they are
///     considered a [`Role::Contributor`]
///   * Otherwise, they are considered a [`Role::Tracker`]
///
/// If `peer` is `Either::Left` then we have the local `PeerId` and we can
/// ignore it for looking at `rad/signed_refs`.
///
/// If `peer` is `Either::Right` then it is a remote peer and we use it for
/// looking at `refs/<remote>/rad/signed_refs`.
pub fn role<S>(
    storage: &S,
    urn: &Urn,
    identity: &SomeIdentity,
    peer: Either<PeerId, PeerId>,
) -> Result<Role, Error>
where
    S: AsRef<storage::ReadOnly>,
{
    let storage = storage.as_ref();
    let role = if is_delegate(identity, urn, peer.into_inner())? {
        Role::Maintainer
    } else if Refs::load(storage, urn, peer.right())?.map_or(false, |refs| !refs.heads.is_empty()) {
        Role::Contributor
    } else {
        Role::Tracker
    };

    Ok(role)
}

fn is_delegate(identity: &SomeIdentity, urn: &Urn, peer: PeerId) -> Result<bool, Error> {
    match identity {
        SomeIdentity::Project(ref project) => {
            Ok(project.delegations().owner(peer.as_public_key()).is_some())
        },
        SomeIdentity::Person(ref person) => Ok(person.delegations().contains(peer.as_public_key())),
        _ => Err(Error::UknownIdentity(urn.clone())),
    }
}

/// Builds the list of tracked peers determining their relation to the `urn`
/// provided.
///
/// If the peer is in the tracking graph but there is no `rad/self` under the
/// tree of remotes, then they have not been replicated, signified by
/// [`Status::NotReplicated`].
///
/// If their `rad/self` is under the tree of remotes, then they have been
/// replicated, signified by [`Status::Replicated`].
pub fn tracked<S>(storage: &S, urn: &Urn) -> Result<Vec<Peer<Status<Person>>>, Error>
where
    S: AsRef<storage::ReadOnly>,
{
    let storage = storage.as_ref();
    let identity = identities::any::get(storage, urn)?
        .ok_or_else(|| identities::Error::NotFound(urn.clone()))?;

    let mut peers = vec![];

    for peer_id in tracking::tracked(storage, urn)? {
        let rad_self = Urn::try_from(Reference::rad_self(Namespace::from(urn.clone()), peer_id))
            .expect("namespace is set");
        let status = if storage.has_urn(&rad_self)? {
            let malkovich = identities::person::get(storage, &rad_self)?
                .ok_or(identities::Error::NotFound(rad_self))?;

            let role = role(storage, urn, &identity, Either::Right(peer_id))?;
            Status::replicated(role, malkovich)
        } else {
            Status::NotReplicated
        };

        peers.push(Peer::Remote { peer_id, status });
    }

    Ok(peers)
}
