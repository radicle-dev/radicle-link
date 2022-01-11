// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryFrom as _;

use crate::{
    git::{
        identities,
        refs::{stored, Refs},
        storage,
        tracking,
        types::{Namespace, Reference},
        Urn,
    },
    identities::{
        relations::{Peer, Status},
        Person,
        SomeIdentity,
    },
    PeerId,
};

#[derive(Debug, thiserror::Error)]
#[allow(clippy::large_enum_variant)]
pub enum Error {
    #[error(transparent)]
    Identities(#[from] identities::Error),
    #[error(transparent)]
    Storage(#[from] storage::Error),
    #[error(transparent)]
    Stored(#[from] stored::Error),
    #[error("the identity `{0}` found is not recognised/supported")]
    UknownIdentity(Urn),
    #[error(transparent)]
    Tracked(#[from] tracking::error::TrackedPeers),
}

/// The `rad/self` under a `Project`/`Person`.
#[derive(Debug, Clone)]
pub struct Persona {
    /// The [`Person`] found at `rad/self`.
    person: Person,
    /// If the peer is a delegate.
    ///
    /// This field being set indicates that the peer has a significant role in
    /// the `Project` or `Person`. This role can be analogised to the term
    /// "maintainer".
    delegate: bool,
    /// The [`Refs`] the peer is advertising.
    ///
    /// This field being set indicates that the peer has a possible interest in
    /// viewing and editing code collaboration artifacts located in this
    /// `Project` or `Person`.
    refs: Option<Refs>,
}

impl Persona {
    /// Load the `Persona` related to the given `identity`. If no `Person` is
    /// found then `None` is returned.
    ///
    /// If `peer` is provided, the `Person` under that peer's `rad/self` is
    /// loaded. Otherwise, the `Person` under the local `rad/self` is
    /// loaded.
    pub fn load<S, Peer>(
        storage: S,
        identity: &SomeIdentity,
        peer: Peer,
    ) -> Result<Option<Self>, Error>
    where
        S: AsRef<storage::ReadOnly>,
        Peer: Into<Option<PeerId>>,
    {
        let storage = storage.as_ref();
        let urn = identity.urn();
        let peer = peer.into();
        let local = storage.peer_id();

        let refs = Refs::load(storage, &urn, peer)?;
        let delegate = is_delegate(identity, peer.unwrap_or(*local))?;
        let rad_self = Urn::try_from(Reference::rad_self(Namespace::from(urn), peer))
            .expect("namespace is set");

        let person = identities::person::get(storage, &rad_self)?;
        Ok(person.map(|person| Self {
            person,
            refs,
            delegate,
        }))
    }

    pub fn person(&self) -> &Person {
        &self.person
    }

    pub fn delegate(&self) -> bool {
        self.delegate
    }

    pub fn refs(&self) -> Option<&Refs> {
        self.refs.as_ref()
    }
}

fn is_delegate(identity: &SomeIdentity, peer: PeerId) -> Result<bool, Error> {
    match identity {
        SomeIdentity::Project(ref project) => {
            Ok(project.delegations().owner(peer.as_public_key()).is_some())
        },
        SomeIdentity::Person(ref person) => Ok(person.delegations().contains(peer.as_public_key())),
        _ => Err(Error::UknownIdentity(identity.urn())),
    }
}

pub type Tracked = Vec<Peer<Status<Persona>>>;

/// Builds the list of tracked peers determining their relation to the `urn`
/// provided.
///
/// If the peer is in the tracking graph but there is no `rad/self` under the
/// tree of remotes, then they have not been replicated, signified by
/// [`Status::NotReplicated`].
///
/// If their `rad/self` is under the tree of remotes, then they have been
/// replicated, signified by [`Status::Replicated`].
pub fn tracked<S>(storage: &S, urn: &Urn) -> Result<Tracked, Error>
where
    S: AsRef<storage::ReadOnly>,
{
    let storage = storage.as_ref();
    let identity = identities::any::get(storage, urn)?
        .ok_or_else(|| identities::Error::NotFound(urn.clone()))?;

    let mut peers = vec![];

    for peer_id in tracking::tracked_peers(storage, Some(urn))? {
        let peer_id = peer_id?;
        let status = match Persona::load(storage, &identity, peer_id)? {
            Some(persona) => Status::replicated(persona),
            None => Status::NotReplicated,
        };
        peers.push(Peer::Remote { peer_id, status });
    }

    Ok(peers)
}
