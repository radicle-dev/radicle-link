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
            sign::{
                self,
                ssh::{self, SshAgent},
            },
            Keystore as _,
        },
        BoxedSigner,
        Signer as _,
        SomeSigner,
    },
    git::storage::ReadOnly,
    profile::Profile,
    Signature,
};

use crate::{keys, runtime};

pub fn signer(profile: &Profile) -> Result<BoxedSigner, super::Error> {
    let storage = ReadOnly::open(profile.paths())?;
    let peer_id = storage.peer_id();
    let pk = (*peer_id.as_public_key()).into();
    tracing::trace!(peer=%peer_id, "obtaining signer for peer");
    let keys = runtime::block_on(ssh::list_keys::<UnixStream>())?;
    if keys.contains(&pk) {
        let agent = SshAgent::new(pk);
        let signer = runtime::block_on(agent.connect::<UnixStream>())?;
        Ok(SomeSigner {
            signer: Arc::new(signer),
        }
        .into())
    } else {
        Err(super::Error::NoSuchKey(*peer_id))
    }
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
    runtime::block_on(ssh::add_key::<UnixStream>(
        key.secret_key.into(),
        constraints,
    ))?;
    Ok(())
}

pub fn remove_signer<C>(profile: &Profile, crypto: C) -> Result<(), super::Error>
where
    C: Crypto,
    C::Error: fmt::Debug + fmt::Display + Send + Sync + 'static,
    C::SecretBox: Serialize + DeserializeOwned,
{
    let store = keys::file_storage(profile, crypto);
    let key = store
        .get_key()
        .map_err(|err| super::Error::GetKey(err.into()))?;
    Ok(runtime::block_on(ssh::remove_key::<UnixStream>(
        &key.public_key.into(),
    ))?)
}

pub fn is_signer_present(profile: &Profile) -> Result<bool, super::Error> {
    let storage = ReadOnly::open(profile.paths())?;
    let peer_id = storage.peer_id();
    let keys = runtime::block_on(ssh::list_keys::<UnixStream>())?;
    Ok(keys.contains(&(*peer_id.as_public_key()).into()))
}

/// Sign the `payload`, using the signing key associated with this `profile`,
/// through the `ssh-agent`.
pub fn sign(profile: &Profile, payload: &[u8]) -> Result<sign::Signature, super::Error> {
    let signer = signer(profile)?;
    Ok(signer.sign_blocking(payload)?)
}

/// Verify the `signature` for the given `payload`, using the public key
/// associated with this `profile`.
pub fn verify(
    profile: &Profile,
    payload: &[u8],
    signature: &Signature,
) -> Result<bool, super::Error> {
    let storage = ReadOnly::open(profile.paths())?;
    let peer_id = storage.peer_id();
    let pk = peer_id.as_public_key();
    Ok(pk.verify(signature, payload))
}
