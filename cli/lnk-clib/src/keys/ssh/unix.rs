// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::fmt;
use std::os::unix::net::UnixStream;
use std::sync::Arc;

use agent::Constraint;
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
        BoxedSignError, BoxedSigner, Signer as _,
    },
    git::storage::ReadOnly,
    keystore::sign::Signer,
    profile::Profile,
    Signature,
};

use crate::keys;

use super::{with_socket, SshAuthSock};

#[derive(Clone)]
pub struct SshSigner {
    signer: Arc<dyn sign::ed25519::Signer<Error = ssh::error::Sign> + Send + Sync>,
}

impl Signer for SshSigner {
    type Error = BoxedSignError;

    fn public_key(&self) -> sign::ed25519::PublicKey {
        self.signer.public_key()
    }

    fn sign(&self, data: &[u8]) -> Result<sign::ed25519::Signature, BoxedSignError> {
        self.signer
            .sign(data)
            .map_err(BoxedSignError::from_std_error)
    }
}

impl librad::Signer for SshSigner {
    fn sign_blocking(&self, data: &[u8]) -> Result<sign::Signature, <Self as sign::Signer>::Error> {
        let data = data.to_vec();
        self.sign(&data)
    }
}

/// Get the signing key associated with this `profile`.
/// See [`SshAuthSock`] for how the `ssh-agent` will be connected to. Use
/// `SshAuthSock::default` to connect via `SSH_AUTH_SOCK`.
pub fn signer(profile: &Profile, sock: SshAuthSock) -> Result<BoxedSigner, super::Error> {
    let storage = ReadOnly::open(profile.paths())?;
    let peer_id = *storage.peer_id();
    let pk = (*peer_id.as_public_key()).into();
    let agent = with_socket(SshAgent::new(pk), sock);
    let keys = ssh::list_keys::<UnixStream>(&agent)?;
    if keys.contains(&pk) {
        let signer = agent.connect::<UnixStream>()?;
        let signer = SshSigner {
            signer: Arc::new(signer),
        };
        Ok(BoxedSigner::new(signer))
    } else {
        Err(super::Error::NoSuchKey(peer_id))
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
    constraints: Vec<Constraint>,
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
    ssh::add_key::<UnixStream>(&agent, key.secret_key.into(), &constraints)?;
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
    Ok(ssh::remove_key::<UnixStream>(
        &agent,
        &key.public_key.into(),
    )?)
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
    let keys = ssh::list_keys::<UnixStream>(&agent)?;
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
