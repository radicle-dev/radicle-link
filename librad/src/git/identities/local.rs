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

use std::ops::Deref;

use thiserror::Error;

use super::{
    super::{
        storage2::{self, config, Storage},
        types::{namespace::Namespace, Force, NamespacedRef},
    },
    user,
};
use crate::{
    identities::git::{Urn, VerifiedUser},
    peer::PeerId,
    signer::Signer,
};

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Validation(#[from] ValidationError),

    #[error(transparent)]
    Identities(#[from] super::Error),

    #[error(transparent)]
    Store(#[from] storage2::Error),

    #[error(transparent)]
    Config(#[from] config::Error),
}

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("identity is not signed by the local key")]
    LocalSignature,

    #[error("identity does not delegate to the local key")]
    LocalDelegation,
}

/// A user identity used as a "user profile" in the context of projects.
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
pub struct LocalIdentity(VerifiedUser);

impl Deref for LocalIdentity {
    type Target = VerifiedUser;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl LocalIdentity {
    pub(super) fn valid<S>(user: VerifiedUser, signer: &S) -> Result<Self, ValidationError>
    where
        S: Signer,
    {
        let local_peer_id = PeerId::from_signer(signer);
        if !user.signatures.contains_key(&local_peer_id) {
            Err(ValidationError::LocalSignature)
        } else if !user.doc.delegations.contains(&local_peer_id) {
            Err(ValidationError::LocalDelegation)
        } else {
            Ok(Self(user))
        }
    }

    /// Link to this [`LocalIdentity`] from `urn`.
    ///
    /// That is, create a symref from `refs/namespaces/<urn>/rad/self` to
    /// `refs/namespaces/<local id>/rad/id`.
    pub fn link<S>(&self, storage: &Storage<S>, from: &Urn) -> Result<(), storage2::Error>
    where
        S: Signer,
    {
        NamespacedRef::rad_id(Namespace::from(self.urn()))
            .symbolic_ref(
                NamespacedRef::rad_self(Namespace::from(from), None),
                Force::True,
            )
            .create(storage.as_raw())
            .and(Ok(()))
            .map_err(storage2::Error::from)
    }

    pub fn into_inner(self) -> VerifiedUser {
        self.0
    }
}

/// Attempt to load a [`LocalIdentity`] from `urn`.
///
/// If the identity could not be found, `None` is returned. If the identity
/// passes verification, is signed by the [`Signer`] of `storage`, and delegates
/// to the key of the [`Signer`], the [`LocalIdentity`] is returned in a `Some`.
pub fn load<S>(storage: &Storage<S>, urn: &Urn) -> Result<Option<LocalIdentity>, Error>
where
    S: Signer,
{
    match user::verify(storage, urn)? {
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
pub fn default<S>(storage: &Storage<S>) -> Result<Option<LocalIdentity>, Error>
where
    S: Signer,
{
    match storage.config()?.user()? {
        Some(urn) => load(storage, &urn),
        None => Ok(None),
    }
}
