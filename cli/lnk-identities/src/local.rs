// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use thiserror::Error;

use librad::git::{
    identities::{local, local::LocalIdentity},
    storage::{self, Storage},
    Urn,
};

use crate::MissingDefaultIdentity;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Config(#[from] storage::config::Error),
    #[error(transparent)]
    Local(#[from] local::Error),
    #[error(transparent)]
    MissingDefault(#[from] MissingDefaultIdentity),
}

pub fn set(storage: &Storage, user: LocalIdentity) -> Result<(), Error> {
    let mut config = storage.config()?;
    Ok(config.set_user(user)?)
}

pub fn get(storage: &Storage, urn: Urn) -> Result<Option<LocalIdentity>, Error> {
    Ok(local::load(storage, urn)?)
}

pub fn default(storage: &Storage) -> Result<LocalIdentity, Error> {
    Ok(local::default(storage)?.ok_or(MissingDefaultIdentity)?)
}
