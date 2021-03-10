// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::ops::Deref;

use thiserror::Error;

use super::{
    super::{
        storage::{self, config, Storage},
        types::{Force, Namespace, Reference},
    },
    person,
};
use crate::{
    identities::git::{Urn, VerifiedPerson},
    peer::PeerId,
    signer::Signer,
};

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Validation(#[from] ValidationError),

    #[error(transparent)]
    Identities(#[from] Box<super::Error>),

    #[error(transparent)]
    Store(#[from] storage::Error),

    #[error(transparent)]
    Config(#[from] config::Error),
}

impl From<super::Error> for Error {
    fn from(e: super::Error) -> Self {
        Self::Identities(Box::new(e))
    }
}

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("identity is not signed by the local key")]
    LocalSignature,

    #[error("identity does not delegate to the local key")]
    LocalDelegation,
}

/// A personal identity used as a "user profile" in the context of projects.
///
/// I.e. the `rad/self` branch.
///
/// This type can only be constructed from an identity, which:
///
/// * Is stored in the local storage
/// * Passes verification
/// * Is signed by the local key
/// * Delegates to the local key
#[derive(Clone, Debug)]
pub struct LocalIdentity(VerifiedPerson);

impl Deref for LocalIdentity {
    type Target = VerifiedPerson;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl LocalIdentity {
    pub(super) fn valid<S>(person: VerifiedPerson, signer: &S) -> Result<Self, ValidationError>
    where
        S: Signer,
    {
        let local_peer_id = PeerId::from_signer(signer);
        if !person.signatures.contains_key(&local_peer_id) {
            Err(ValidationError::LocalSignature)
        } else if !person.delegations().contains(&local_peer_id) {
            Err(ValidationError::LocalDelegation)
        } else {
            Ok(Self(person))
        }
    }

    /// Link to this [`LocalIdentity`] from `urn`.
    ///
    /// That is, create a symref from `refs/namespaces/<urn>/rad/self` to
    /// `refs/namespaces/<local id>/rad/id`.
    pub fn link(&self, storage: &Storage, from: &Urn) -> Result<(), storage::Error> {
        Reference::rad_id(Namespace::from(self.urn()))
            .symbolic_ref(
                Reference::rad_self(Namespace::from(from), None),
                Force::True,
            )
            .create(storage.as_raw())
            .and(Ok(()))
            .map_err(storage::Error::from)
    }

    pub fn into_inner(self) -> VerifiedPerson {
        self.0
    }
}

/// Attempt to load a [`LocalIdentity`] from `urn`.
///
/// The [`Urn::path`] is always set to `rad/self`.
///
/// If the identity could not be found, `None` is returned. If the identity
/// passes verification, is signed by the [`Signer`] of `storage`, and delegates
/// to the key of the [`Signer`], the [`LocalIdentity`] is returned in a `Some`.
#[tracing::instrument(level = "debug", skip(storage), err)]
pub fn load(storage: &Storage, urn: Urn) -> Result<Option<LocalIdentity>, Error> {
    let urn = urn.with_path(reflike!("refs/rad/self"));
    tracing::debug!("loading local id from {}", urn);
    match person::verify(storage, &urn)? {
        Some(verified) => Ok(Some(LocalIdentity::valid(verified, storage.signer())?)),
        None => Ok(None),
    }
}

/// Attempt to load a pre-configured [`LocalIdentity`].
///
/// A default [`LocalIdentity`] can be configured via
/// [`config::Config::set_user`].
///
/// If no default identity was configured, `None` is returned. Otherwise, the
/// result is the result of calling [`load`] with the pre-configured [`Urn`].
#[tracing::instrument(level = "debug", skip(storage), err)]
pub fn default(storage: &Storage) -> Result<Option<LocalIdentity>, Error> {
    match storage.config()?.user()? {
        Some(urn) => load(storage, urn),
        None => Ok(None),
    }
}
