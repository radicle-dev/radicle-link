// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{fmt, sync::Arc};

use serde::{de::DeserializeOwned, Serialize};
use thrussh_agent::{client::tokio::UnixStream, Constraint};

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
    git::storage::ReadOnly,
    profile::Profile,
};

use crate::{keys, runtime};

pub fn signer(profile: &Profile) -> Result<BoxedSigner, super::Error> {
    let storage = ReadOnly::open(profile.paths())?;
    let peer_id = storage.peer_id();
    tracing::trace!(peer=%peer_id, "obtaining signer for peer");
    let agent = SshAgent::new((*peer_id.as_public_key()).into());
    let signer = runtime::block_on(agent.connect::<UnixStream>())?;
    Ok(SomeSigner {
        signer: Arc::new(signer),
    }
    .into())
}

pub fn add_signer<C>(
    profile: &Profile,
    crypto: C,
    constraints: &[Constraint],
) -> Result<(), super::Error>
where
    C: Crypto,
    C::Error: fmt::Debug + fmt::Display + Send + Sync + 'static,
    C::SecretBox: Serialize + DeserializeOwned,
{
    let store = keys::file_storage(profile, crypto);
    let key = store
        .get_key()
        .map_err(|err| super::Error::GetKey(err.into()))?;
    Ok(runtime::block_on(ssh::add_key::<UnixStream>(
        key.secret_key.into(),
        constraints,
    ))?)
}
