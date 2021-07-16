// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{error, fmt};

use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

use librad::{
    git::storage::{self, read, ReadOnly, Storage},
    keys::{IntoSecretKeyError, PublicKey, SecretKey},
    paths::Paths,
    peer::PeerId,
    profile::{self, Profile, ProfileId, RadHome},
};
use radicle_keystore::{crypto::Crypto, file, FileStorage, Keystore as _};

pub mod cli;

const KEY_FILE: &str = "librad.key";

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
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

fn file_storage<C>(profile: &Profile, crypto: C) -> FileStorage<C, PublicKey, SecretKey, ()>
where
    C: Crypto,
{
    FileStorage::new(&profile.paths().keys_dir().join(KEY_FILE), crypto)
}

fn get_or_active<P>(home: &RadHome, id: P) -> Result<Profile, Error>
where
    P: Into<Option<ProfileId>>,
{
    match id.into() {
        Some(id) => Profile::get(&home, id.clone())?.ok_or_else(|| Error::NoProfile(id)),
        None => Profile::active(&home)?.ok_or(Error::NoActiveProfile),
    }
}

/// Initialise a [`Profile`], generating a new [`SecretKey`] and [`Storage`].
pub fn create<C: Crypto>(crypto: C) -> Result<(Profile, PeerId), Error>
where
    C::Error: fmt::Debug + fmt::Display + Send + Sync + 'static,
    C::SecretBox: Serialize + DeserializeOwned,
{
    let home = RadHome::new();
    let profile = Profile::new(&home)?;
    Profile::set(&home, profile.id().clone())?;
    let key = SecretKey::new();
    let mut store: FileStorage<C, PublicKey, SecretKey, _> = file_storage(&profile, crypto);
    store.put_key(key.clone())?;
    Storage::open(profile.paths(), key.clone())?;

    Ok((profile, PeerId::from(key)))
}

/// Get the current active `ProfileId`.
pub fn get() -> Result<Option<Profile>, Error> {
    let home = RadHome::new();
    Profile::active(&home).map_err(Error::from)
}

/// Set the active profile to the given `ProfileId`.
pub fn set(id: ProfileId) -> Result<(), Error> {
    let home = RadHome::new();
    Profile::set(&home, id).map_err(Error::from).map(|_| ())
}

/// List the set of active profiles that exist.
pub fn list() -> Result<Vec<Profile>, Error> {
    let home = RadHome::new();
    Profile::list(&home).map_err(Error::from)
}

/// Get the `PeerId` associated to the given [`ProfileId`]
pub fn peer_id<P>(id: P) -> Result<PeerId, Error>
where
    P: Into<Option<ProfileId>>,
{
    let home = RadHome::new();
    let profile = get_or_active(&home, id)?;
    let read = ReadOnly::open(profile.paths())?;
    Ok(*read.peer_id())
}

pub fn paths<P>(id: P) -> Result<Paths, Error>
where
    P: Into<Option<ProfileId>>,
{
    let home = RadHome::new();
    get_or_active(&home, id).map(|p| p.paths().clone())
}

/// Add a profile's [`SecretKey`] to the `ssh-agent`.
pub fn ssh_add<P, C>(id: P, crypto: C) -> Result<(ProfileId, PeerId), Error>
where
    C: Crypto,
    C::Error: fmt::Debug + fmt::Display + Send + Sync + 'static,
    C::SecretBox: Serialize + DeserializeOwned,
    P: Into<Option<ProfileId>>,
{
    let home = RadHome::new();
    let profile = get_or_active(&home, id)?;
    let store = file_storage(&profile, crypto);
    let key = store.get_key()?;
    let peer_id = PeerId::from(key.public_key);
    println!("TODO: {}", peer_id);

    Ok((profile.id().clone(), peer_id))
}
