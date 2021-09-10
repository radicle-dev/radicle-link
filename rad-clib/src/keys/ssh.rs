// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{fmt, sync::Arc};

use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;
use thrussh_agent::{client::ClientStream, Constraint};

use librad::{
    crypto::{
        keystore::{
            crypto::Crypto,
            sign::ssh::{self, SshAgent},
            Keystore as _,
        },
        BoxedSigner,
        SomeSigner,
    },
    git::storage::{read, ReadOnly},
    profile::Profile,
};

use crate::runtime;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    SshConnect(#[from] ssh::error::Connect),
    #[error(transparent)]
    AddKey(#[from] ssh::error::AddKey),
    #[error(transparent)]
    GetKey(#[from] Box<dyn std::error::Error + Send + Sync + 'static>),
    #[error(transparent)]
    StorageInit(#[from] read::error::Init),
}

pub fn signer<S>(profile: &Profile) -> Result<BoxedSigner, Error>
where
    S: ClientStream + Unpin + 'static,
{
    let storage = ReadOnly::open(profile.paths())?;
    let peer_id = storage.peer_id();
    tracing::trace!(peer=%peer_id, "obtaining signer for peer");
    let agent = SshAgent::new((*peer_id.as_public_key()).into());
    let signer = runtime::block_on(agent.connect::<S>())?;
    Ok(SomeSigner {
        signer: Arc::new(signer),
    }
    .into())
}

pub fn add_signer<S, C>(
    profile: &Profile,
    crypto: C,
    constraints: &[Constraint],
) -> Result<(), Error>
where
    C: Crypto,
    C::Error: fmt::Debug + fmt::Display + Send + Sync + 'static,
    C::SecretBox: Serialize + DeserializeOwned,
    S: ClientStream + Unpin + 'static,
{
    let store = super::file_storage(profile, crypto);
    let key = store.get_key().map_err(|err| Error::GetKey(err.into()))?;
    Ok(runtime::block_on(ssh::add_key::<S>(
        key.secret_key.into(),
        constraints,
    ))?)
}
