// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{fmt, sync::Arc};

use lnk_thrussh_agent::{client::tokio::UnixStream, Constraint};
use serde::{de::DeserializeOwned, Serialize};

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

use super::{with_socket, SshAuthSock};

/// Get the signing key associated with this `profile`.
/// See [`SshAuthSock`] for how the `ssh-agent` will be connected to. Use
/// `SshAuthSock::default` to connect via `SSH_AUTH_SOCK`.
pub fn signer(profile: &Profile, sock: SshAuthSock) -> Result<BoxedSigner, super::Error> {
    let storage = ReadOnly::open(profile.paths())?;
    let peer_id = storage.peer_id();
    let pk = (*peer_id.as_public_key()).into();
    let agent = with_socket(SshAgent::new(pk), sock);
    tracing::trace!(peer=%peer_id, "obtaining signer for peer");
    let keys = runtime::block_on(ssh::list_keys::<UnixStream>(&agent))?;
    if keys.contains(&pk) {
        let signer = runtime::block_on(agent.connect::<UnixStream>())?;
        Ok(SomeSigner {
            signer: Arc::new(signer),
        }
        .into())
    } else {
        Err(super::Error::NoSuchKey(*peer_id))
    }
}

/// Add the signing key associated with this `profile` to the `ssh-agent`.
///
/// See [`SshAuthSock`] for how the agent will be connected to. Use
/// `SshAuthSock::default` to connect via `SSH_AUTH_SOCK`.
///
/// The `crypto` passed will decide how the key storage is unlocked.
pub fn add_signer<C>(
    profile: &Profile,
    sock: SshAuthSock,
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
    let agent = with_socket(SshAgent::new(key.public_key.into()), sock);
    runtime::block_on(ssh::add_key::<UnixStream>(
        &agent,
        key.secret_key.into(),
        constraints,
    ))?;
    Ok(())
}

/// Remove the signing key associated with this `profile` from the `ssh-agent`.
///
/// See [`SshAuthSock`] for how the agent will be connected to. Use
/// `SshAuthSock::default` to connect via `SSH_AUTH_SOCK`.
///
/// The `crypto` passed will decide how the key storage is unlocked.
pub fn remove_signer<C>(profile: &Profile, sock: SshAuthSock, crypto: C) -> Result<(), super::Error>
where
    C: Crypto,
    C::Error: fmt::Debug + fmt::Display + Send + Sync + 'static,
    C::SecretBox: Serialize + DeserializeOwned,
{
    let store = keys::file_storage(profile, crypto);
    let key = store
        .get_key()
        .map_err(|err| super::Error::GetKey(err.into()))?;
    let agent = with_socket(SshAgent::new(key.public_key.into()), sock);
    Ok(runtime::block_on(ssh::remove_key::<UnixStream>(
        &agent,
        &key.public_key.into(),
    ))?)
}

/// Test whether the signing key associated with this `profile` is present on
/// the `ssh-agent`.
///
/// See [`SshAuthSock`] for how the agent will be connected to. Use
/// `SshAuthSock::default` to connect via `SSH_AUTH_SOCK`.
pub fn is_signer_present(profile: &Profile, sock: SshAuthSock) -> Result<bool, super::Error> {
    let storage = ReadOnly::open(profile.paths())?;
    let peer_id = storage.peer_id();
    let pk = (*peer_id.as_public_key()).into();
    let agent = with_socket(SshAgent::new(pk), sock);
    let keys = runtime::block_on(ssh::list_keys::<UnixStream>(&agent))?;
    Ok(keys.contains(&pk))
}

/// Sign the `payload`, using the signing key associated with this `profile`,
/// through the `ssh-agent`.
///
/// See [`SshAuthSock`] for how the agent will be connected to. Use
/// `SshAuthSock::default` to connect via `SSH_AUTH_SOCK`.
pub fn sign(
    profile: &Profile,
    sock: SshAuthSock,
    payload: &[u8],
) -> Result<sign::Signature, super::Error> {
    let signer = signer(profile, sock)?;
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
