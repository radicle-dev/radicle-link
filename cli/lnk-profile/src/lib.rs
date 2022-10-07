// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{error, fmt};

use agent::Constraint;
use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

use librad::{
    crypto::{
        keystore::{crypto::Crypto, file, FileStorage, Keystore as _},
        IntoSecretKeyError, PeerId, PublicKey, SecretKey,
    },
    git::storage::{self, read, ReadOnly, Storage},
    paths::Paths,
    profile::{self, LnkHome, Profile, ProfileId},
    Signature,
};
use lnk_clib::keys::{self, ssh::SshAuthSock};

pub mod cli;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error(transparent)]
    AddKey(#[from] keys::ssh::Error),
    #[error(transparent)]
    Keystore(Box<dyn error::Error + Send + Sync + 'static>),
    #[error("no active profile was found, perhaps you need to create one")]
    NoActiveProfile,
    #[error("no profile was found for `{0}`")]
    NoProfile(ProfileId),
    #[error(transparent)]
    Profile(#[from] profile::Error),
    #[error(transparent)]
    Storage(#[from] storage::error::Init),
    #[error(transparent)]
    ReadOnly(#[from] read::error::Init),
}

impl<C> From<file::Error<C, IntoSecretKeyError>> for Error
where
    C: fmt::Debug + fmt::Display + Send + Sync + 'static,
{
    fn from(err: file::Error<C, IntoSecretKeyError>) -> Self {
        Self::Keystore(Box::new(err))
    }
}

fn get_or_active<P>(home: &LnkHome, id: P) -> Result<Profile, Error>
where
    P: Into<Option<ProfileId>>,
{
    match id.into() {
        Some(id) => Profile::get(home, id.clone())?.ok_or(Error::NoProfile(id)),
        None => Profile::active(home)?.ok_or(Error::NoActiveProfile),
    }
}

/// Initialise a [`Profile`], generating a new [`SecretKey`] and [`Storage`].
pub fn create<H, C: Crypto>(home: H, crypto: C) -> Result<(Profile, PeerId), Error>
where
    H: Into<Option<LnkHome>>,
    C::Error: fmt::Debug + fmt::Display + Send + Sync + 'static,
    C::SecretBox: Serialize + DeserializeOwned,
{
    let home = home.into().unwrap_or_default();
    let profile = Profile::new(&home)?;
    Profile::set(&home, profile.id().clone())?;
    let key = SecretKey::new();
    let mut store: FileStorage<C, PublicKey, SecretKey, _> = keys::file_storage(&profile, crypto);
    store.put_key(key.clone())?;
    Storage::open(profile.paths(), key.clone())?;

    Ok((profile, PeerId::from(key)))
}

/// Get the current active `ProfileId`.
pub fn get<H>(home: H, id: Option<ProfileId>) -> Result<Option<Profile>, Error>
where
    H: Into<Option<LnkHome>>,
{
    let home = home.into().unwrap_or_default();
    match id {
        Some(id) => Profile::get(&home, id).map_err(Error::from),
        None => Profile::active(&home).map_err(Error::from),
    }
}

/// Set the active profile to the given `ProfileId`.
pub fn set<H>(home: H, id: ProfileId) -> Result<(), Error>
where
    H: Into<Option<LnkHome>>,
{
    let home = home.into().unwrap_or_default();
    Profile::set(&home, id).map_err(Error::from).map(|_| ())
}

/// List the set of active profiles that exist.
pub fn list<H>(home: H) -> Result<Vec<Profile>, Error>
where
    H: Into<Option<LnkHome>>,
{
    let home = home.into().unwrap_or_default();
    Profile::list(&home).map_err(Error::from)
}

/// Get the `PeerId` associated to the given [`ProfileId`]
pub fn peer_id<H, P>(home: H, id: P) -> Result<PeerId, Error>
where
    H: Into<Option<LnkHome>>,
    P: Into<Option<ProfileId>>,
{
    let home = home.into().unwrap_or_default();
    let profile = get_or_active(&home, id)?;
    let read = ReadOnly::open(profile.paths())?;
    Ok(*read.peer_id())
}

pub fn paths<H, P>(home: H, id: P) -> Result<Paths, Error>
where
    H: Into<Option<LnkHome>>,
    P: Into<Option<ProfileId>>,
{
    let home = home.into().unwrap_or_default();
    get_or_active(&home, id).map(|p| p.paths().clone())
}

/// Add a profile's [`SecretKey`] to the `ssh-agent`.
pub fn ssh_add<H, P, C>(
    home: H,
    id: P,
    sock: SshAuthSock,
    crypto: C,
    constraints: Vec<Constraint>,
) -> Result<ProfileId, Error>
where
    H: Into<Option<LnkHome>>,
    C: Crypto,
    C::Error: fmt::Debug + fmt::Display + Send + Sync + 'static,
    C::SecretBox: Serialize + DeserializeOwned,
    P: Into<Option<ProfileId>>,
{
    let home = home.into().unwrap_or_default();
    let profile = get_or_active(&home, id)?;
    keys::ssh::add_signer(&profile, sock, crypto, constraints)?;
    Ok(profile.id().clone())
}

/// Remove a profile's [`SecretKey`] from the `ssh-agent`.
pub fn ssh_remove<H, P, C>(home: H, id: P, sock: SshAuthSock, crypto: C) -> Result<ProfileId, Error>
where
    H: Into<Option<LnkHome>>,
    C: Crypto,
    C::Error: fmt::Debug + fmt::Display + Send + Sync + 'static,
    C::SecretBox: Serialize + DeserializeOwned,
    P: Into<Option<ProfileId>>,
{
    let home = home.into().unwrap_or_default();
    let profile = get_or_active(&home, id)?;
    keys::ssh::remove_signer(&profile, sock, crypto)?;
    Ok(profile.id().clone())
}

/// See if a profile's [`SecretKey`] is present in the `ssh-agent`.
pub fn ssh_ready<H, P>(home: H, id: P, sock: SshAuthSock) -> Result<(ProfileId, bool), Error>
where
    H: Into<Option<LnkHome>>,
    P: Into<Option<ProfileId>>,
{
    let home = home.into().unwrap_or_default();
    let profile = get_or_active(&home, id)?;
    let present = keys::ssh::is_signer_present(&profile, sock)?;
    Ok((profile.id().clone(), present))
}

/// Sign a payload with a profile's [`SecretKey`] from the `ssh-agent`.
pub fn ssh_sign<H, P>(
    home: H,
    id: P,
    sock: SshAuthSock,
    payload: String,
) -> Result<(ProfileId, Signature), Error>
where
    H: Into<Option<LnkHome>>,
    P: Into<Option<ProfileId>>,
{
    let home = home.into().unwrap_or_default();
    let profile = get_or_active(&home, id)?;
    let sig = keys::ssh::sign(&profile, sock, payload.as_bytes())?;
    Ok((profile.id().clone(), sig.into()))
}

/// Verify a signature and payload with a profile's [`PublicKey`].
pub fn ssh_verify<H, P>(
    home: H,
    id: P,
    payload: String,
    signature: Signature,
) -> Result<(ProfileId, bool), Error>
where
    H: Into<Option<LnkHome>>,
    P: Into<Option<ProfileId>>,
{
    let home = home.into().unwrap_or_default();
    let profile = get_or_active(&home, id)?;
    let verified = keys::ssh::verify(&profile, payload.as_bytes(), &signature)?;
    Ok((profile.id().clone(), verified))
}
